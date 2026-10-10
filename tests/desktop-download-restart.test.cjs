// Real WebView2 native callbacks, isolated loopback server, owned app processes.
const http=require('node:http'),fs=require('node:fs'),path=require('node:path');
const {spawn}=require('node:child_process'),{randomUUID,createHash}=require('node:crypto');
const {DatabaseSync}=require('node:sqlite'),assert=require('node:assert/strict');
const root=path.resolve(__dirname,'../runtime/tauri-smoke');
const binary=process.env.DESKTOP_TEST_BINARY||path.resolve(__dirname,'../desktop/src-tauri/target/debug/library-workspace.exe');
const txt=Buffer.from('TI\tAU\tAF\tSO\tPY\tC1\tUT\tDI\r\nSynthetic paper\tTest, A\tAlice Test\tSynthetic Journal\t2026\tShanghai Jiao Tong Univ\tWOS:000123456789012\t10.1234/test\r\n');
function snapshot(folder){
  const db=new DatabaseSync(path.join(folder,'workspace.sqlite3'),{readOnly:true});
  try{db.exec('PRAGMA busy_timeout=5000; BEGIN');return Object.fromEntries(['tasks','native_downloads','download_queues','attempts'].map(table=>[table,db.prepare(`SELECT data FROM ${table} ORDER BY rowid`).all().map(r=>JSON.parse(r.data))]));}finally{db.close();}
}
async function kill(child){
  if(child.exitCode!==null||child.signalCode!==null)return;
  const killer=spawn('taskkill',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'pipe'});
  const code=await new Promise((resolve,reject)=>{killer.once('exit',resolve);killer.once('error',reject);});assert.equal(code,0);
}
(async()=>{
  let phase='',held=new Set(),requests=[],runs=[];
  const base=fs.readFileSync(path.join(__dirname,'fixtures/wos.html'),'utf8');
  const server=http.createServer((req,res)=>{
    if(req.url==='/receipt/search'||req.url==='/receipt/export'){
      requests.push({phase,path:req.url});res.end('{}');return;
    }
    if(req.url==='/wos/export.txt'){
      requests.push({phase,path:req.url});
      // Chromium sniffs the first bytes before reporting a download. Send a
      // prefix beyond its sniff buffer, while withholding the declared body.
      res.writeHead(200,{'Content-Type':'text/plain','Content-Disposition':'attachment; filename="native-receipt.txt"','Content-Length':131072});
      res.write(Buffer.concat([txt,Buffer.alloc(4096,32)]));held.add(res);return;
    }
    const fixture=base.replace('searches++;history.pushState',"fetch('/receipt/search');searches++;history.pushState")
      .replace('exportsMade++;',"fetch('/receipt/export');exportsMade++;")
      .replace('a.href=URL.createObjectURL',phase==='download-requested'?"a.href=location.origin+'/wos/export.txt';a.dataset.ignored=URL.createObjectURL":'a.href=URL.createObjectURL');
    res.writeHead(200,{'Content-Type':'text/html; charset=utf-8'});res.end(fixture);
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const origin=`http://127.0.0.1:${server.address().port}/`;
  async function run(id,next,crash){
    phase=next;const folder=path.join(root,id);
    const env={...process.env,DESKTOP_SMOKE_ORIGIN:origin,DESKTOP_SMOKE_RESTART_ID:id,DESKTOP_SMOKE_RESTART_PHASE:next};
    delete env.DESKTOP_SMOKE_LOSE_UPLOAD_RESPONSE;
    const child=spawn(binary,[],{env,windowsHide:true,stdio:'pipe'});let output='',ready;
    const boundary=new Promise(resolve=>ready=resolve);
    const exited=new Promise((resolve,reject)=>{child.once('exit',(code,signal)=>resolve({code,signal}));child.once('error',reject);});
    for(const stream of [child.stdout,child.stderr])stream.on('data',b=>{
      output+=b;process.stdout.write(b);
      if(output.includes('DOWNLOAD_COMPLETE_BEFORE_ADOPTION')||(next==='download-requested'&&output.includes('Download accepted:')))ready();
    });
    let timer;const timed=new Promise((_,reject)=>timer=setTimeout(()=>reject(Error(`${next} timeout: ${output}`)),150000));
    try{
      if(crash){
        await Promise.race([boundary,exited.then(r=>{throw Error(`Exited before crash: ${JSON.stringify(r)} ${output}`);}),timed]);
        const before=snapshot(folder),r=before.native_downloads[0];
        assert.equal(before.tasks.length,1);assert.equal(before.attempts.length,0);assert.equal(before.tasks[0].artifact,null);
        assert.equal(before.tasks[0].stage,'downloading');assert.equal(before.download_queues[0].cursor,0);assert.equal(before.download_queues[0].status,'running');
        assert.equal(r.state,next==='download-complete'?'completed':'requested');assert.equal(r.task_id,'receipt-paper');
        if(next==='download-complete'){
          assert.equal(r.sha256,createHash('sha256').update(txt).digest('hex'));assert.equal(r.bytes,txt.length);
          assert.deepEqual(fs.readFileSync(r.path),txt);assert.ok(r.finished);
        }else{assert.equal(r.sha256,null);assert.equal(r.finished,null);}
        fs.writeFileSync(path.join(folder,`download-boundary-${next}.json`),JSON.stringify({process_id:child.pid,local:before,requests},null,2));
        await kill(child);await Promise.race([exited,timed]);runs.push({phase:next,process_id:child.pid,terminated:true});
      }else{
        const exit=await Promise.race([exited,timed]);
        const report=JSON.parse(fs.readFileSync(path.join(folder,`restart-${next}.json`),'utf8'));
        assert.equal(exit.code,0,JSON.stringify(report.failure));assert.equal(report.passed,true,JSON.stringify(report.failure));
        assert.equal(report.process_id,child.pid);runs.push({phase:next,process_id:child.pid,passed:true});
      }
    }finally{clearTimeout(timer);await kill(child);for(const response of held)response.destroy();held.clear();}
    return folder;
  }
  try{
    const id=randomUUID();await run(id,'download-complete',true);const folder=await run(id,'download-complete-resume',false);
    const before=JSON.parse(fs.readFileSync(path.join(folder,'download-boundary-download-complete.json'),'utf8'));
    const after=snapshot(folder),r=after.native_downloads[0],a=after.tasks[0].artifact;
    assert.equal(r.id,before.local.native_downloads[0].id);assert.equal(r.state,'adopted');assert.equal(r.path,before.local.native_downloads[0].path);
    assert.deepEqual(fs.readFileSync(a.path),txt);assert.equal(a.candidate.sha256,r.sha256);assert.equal(a.record_url,r.record_url);
    assert.equal(after.tasks[0].stage,'downloaded');assert.equal(after.download_queues[0].cursor,1);assert.equal(after.download_queues[0].status,'completed');
    assert.equal(fs.readdirSync(path.join(folder,'downloads')).length,1);assert.equal(after.attempts.length,0);
    const second=randomUUID();await run(second,'download-requested',true);const unknownFolder=await run(second,'download-requested-resume',false);
    const unknown=snapshot(unknownFolder);assert.equal(unknown.native_downloads.length,1);assert.equal(unknown.native_downloads[0].state,'requested');
    assert.equal(unknown.tasks[0].artifact,null);assert.equal(unknown.tasks[0].last_error.code,'DOWNLOAD_RESULT_UNKNOWN');assert.equal(unknown.attempts.length,0);
    assert.equal(new Set(runs.map(r=>r.process_id)).size,4);
    assert.deepEqual(requests.filter(r=>r.path==='/receipt/export').map(r=>r.phase),['download-complete','download-requested']);
    assert.deepEqual(requests.filter(r=>r.path==='/receipt/search').map(r=>r.phase),['download-complete','download-requested']);
    assert.equal(requests.some(r=>r.phase.endsWith('resume')),false,'Restart must neither search nor export either sample');
    const evidence={passed:true,synthetic:true,full_app_and_webview_restart:true,runs,requests,completed_workspace:folder,unconfirmed_workspace:unknownFolder};
    fs.writeFileSync(path.join(folder,'download-restart-acceptance.json'),JSON.stringify(evidence,null,2));
    console.log('PASS real native Finished persisted before artifact save: restart adopts original bytes/hash/source and completes original queue without reopening WOS or downloading again.');
    console.log('PASS native Requested without Finished: restart preserves unknown receipt, partial file never adopted, no automatic search/export or platform writes.');
    console.log('Evidence:',path.join(folder,'download-restart-acceptance.json'));
  }finally{for(const response of held)response.destroy();server.close();server.closeAllConnections();}
})().catch(error=>{console.error(error);process.exitCode=1;});
