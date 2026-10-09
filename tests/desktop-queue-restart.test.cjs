// Harness owns the app processes and a loopback-only WOS, never a user profile.
const http=require('node:http'),fs=require('node:fs'),path=require('node:path');
const {spawn}=require('node:child_process'),{randomUUID}=require('node:crypto');
const {DatabaseSync}=require('node:sqlite');
const assert=require('node:assert/strict');
const root=path.resolve(__dirname,'../runtime/tauri-smoke');
const binary=process.env.DESKTOP_TEST_BINARY||path.resolve(__dirname,'../desktop/src-tauri/target/debug/library-workspace.exe');
function snapshot(folder){
  const db=new DatabaseSync(path.join(folder,'workspace.sqlite3'),{readOnly:true});
  try {db.exec('PRAGMA busy_timeout=5000; BEGIN');return {queues:db.prepare('SELECT data FROM download_queues ORDER BY rowid').all().map(r=>JSON.parse(r.data)),
    tasks:db.prepare('SELECT data FROM tasks ORDER BY rowid').all().map(r=>JSON.parse(r.data)),
    attempts:db.prepare('SELECT * FROM attempts').all()};}finally{db.close();}
}
(async()=>{
  let phase='',boundary,queries=[],held=new Set(),runs=[];
  const fixture=fs.readFileSync(path.join(__dirname,'fixtures/wos.html'),'utf8')
    .replace("main.querySelector('button').onclick=()=>{searches++;history.pushState",
      "main.querySelector('button').onclick=async()=>{const query=main.querySelector('input').value;await fetch('/queue/search',{method:'POST',body:JSON.stringify({query})});if(query.startsWith('Missing')){main.insertAdjacentHTML('afterbegin','<p>Your search found no results</p>');return;}searches++;history.pushState");
  assert.ok(fixture.includes("fetch('/queue/search'"),'The real search fixture must be instrumented');
  for(const script of fixture.matchAll(/<script>([\s\S]*?)<\/script>/g))new(require('node:vm').Script)(script[1]);
  const server=http.createServer((req,res)=>{
    if(req.url==='/queue/search'){
      let body='';req.on('data',b=>body+=b);req.on('end',()=>{
        const query=JSON.parse(body).query;queries.push({phase,query});
        if(phase==='queue-start'&&query==='10.1234/test'){held.add(res);boundary?.();return;}
        const reply=()=>{if(!res.destroyed){res.writeHead(200);res.end('{}');}};
        if(phase==='queue-pause')setTimeout(reply,600);else reply();
      });return;
    }
    res.writeHead(200,{'Content-Type':'text/html; charset=utf-8'});res.end(fixture);
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const origin=`http://127.0.0.1:${server.address().port}/`;
  async function run(id,next,crash){
    phase=next;const folder=path.join(root,id);
    let reach;const ready=new Promise(resolve=>reach=resolve);boundary=reach;
    const env={...process.env,DESKTOP_SMOKE_ORIGIN:origin,DESKTOP_SMOKE_RESTART_ID:id,DESKTOP_SMOKE_RESTART_PHASE:next};
    delete env.DESKTOP_SMOKE_LOSE_UPLOAD_RESPONSE;
    const child=spawn(binary,[],{env,windowsHide:true,stdio:'pipe'});let output='';
    const exited=new Promise((resolve,reject)=>{child.once('exit',(code,signal)=>resolve({code,signal}));child.once('error',reject);});
    for(const stream of [child.stdout,child.stderr])stream.on('data',b=>{output+=b;process.stdout.write(b);if(output.includes('QUEUE_PAUSE_READY'))reach();});
    let timer;const timed=new Promise((_,reject)=>timer=setTimeout(()=>reject(Error(`Queue ${next} timeout: ${output}`)),160000));
    try {
      if(crash){
        await Promise.race([ready,exited.then(r=>{throw Error(`Exited before ${next} crash: ${JSON.stringify(r)} ${output}`);}),timed]);
        const before=snapshot(folder),q=before.queues.at(-1);
        assert.equal(q.owner,'queue-fixture');assert.equal(before.attempts.length,0,'Download queue must never submit a platform write');
        if(next==='queue-start'){
          assert.equal(q.status,'running');assert.equal(q.cursor,3);assert.equal(q.targets.length,7);
          assert.deepEqual(q.outcomes.map(o=>o.error.code),['NO_RESULT','NO_RESULT','NO_RESULT']);
          assert.equal(before.tasks.find(t=>t.id==='queue-4').stage,'searching');
          assert.equal(before.tasks.find(t=>t.id==='queue-4').running,true);
        }else{
          assert.equal(q.status,'paused');assert.equal(q.cursor,0);assert.equal(q.pause_requested,true);
          assert.equal(before.tasks[0].artifact,null);assert.equal(queries.filter(r=>r.phase===next).length,1);
        }
        fs.writeFileSync(path.join(folder,`queue-boundary-${next}.json`),JSON.stringify({process_id:child.pid,local:before,queries},null,2));
        const killer=spawn('taskkill',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'pipe'});
        const killed=await new Promise((resolve,reject)=>{killer.once('exit',resolve);killer.once('error',reject);});
        assert.equal(killed,0);await Promise.race([exited,timed]);
        assert.equal(fs.existsSync(path.join(folder,`restart-${next}.json`)),false);
        runs.push({phase:next,process_id:child.pid,terminated:true});
      }else{
        const exit=await Promise.race([exited,timed]);
        const report=JSON.parse(fs.readFileSync(path.join(folder,`restart-${next}.json`),'utf8'));
        assert.equal(exit.code,0,JSON.stringify(report.failure));assert.equal(report.passed,true,JSON.stringify(report.failure));
        assert.equal(report.process_id,child.pid);runs.push({phase:next,process_id:child.pid,passed:true});
      }
    }finally{
      clearTimeout(timer);boundary=null;
      if(child.exitCode===null&&child.signalCode===null){
        const cleanup=spawn('taskkill',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'pipe'});
        await new Promise(resolve=>{cleanup.once('exit',resolve);cleanup.once('error',resolve);});
      }
      for(const res of held)res.destroy();held.clear();
    }
    return folder;
  }
  try {
    const id=randomUUID();await run(id,'queue-start',true);const folder=await run(id,'queue-resume',false);
    const before=JSON.parse(fs.readFileSync(path.join(folder,'queue-boundary-queue-start.json'),'utf8'));
    const after=snapshot(folder),q=after.queues[0];
    assert.equal(q.id,before.local.queues[0].id);assert.deepEqual(q.targets,before.local.queues[0].targets);
    assert.deepEqual(q.outcomes.slice(0,3),before.local.queues[0].outcomes);
    assert.equal(q.status,'completed');assert.equal(q.cursor,7);
    assert.equal(after.queues[1].owner,'other-owner');assert.equal(after.queues[1].status,'completed');
    assert.deepEqual(queries.map(r=>r.query),['Missing 1','Missing 2','Missing 3','10.1234/test','10.1234/test','Missing other']);
    assert.equal(queries.some(r=>/fifth|after-restart/.test(r.query)),false);
    assert.equal(after.tasks.find(t=>t.id==='queue-new').artifact,null);
    assert.equal(fs.readdirSync(path.join(folder,'downloads')).length,1);
    assert.equal(after.attempts.length,0);
    const crashQueries=structuredClone(queries);queries=[];
    const pauseId=randomUUID();await run(pauseId,'queue-pause',true);const pauseFolder=await run(pauseId,'queue-paused-resume',false);
    const pauseBoundary=JSON.parse(fs.readFileSync(path.join(pauseFolder,'queue-boundary-queue-pause.json'),'utf8'));
    const paused=snapshot(pauseFolder),pq=paused.queues[0];
    assert.equal(pq.id,pauseBoundary.local.queues[0].id);assert.deepEqual(pq.targets,pauseBoundary.local.queues[0].targets);
    assert.equal(pq.status,'completed');assert.equal(pq.cursor,2);assert.equal(pq.outcomes[1].error.code,'NO_RESULT');
    assert.deepEqual(queries.map(r=>r.query),['10.1234/test','10.1234/test','Missing after pause']);
    assert.equal(fs.readdirSync(path.join(pauseFolder,'downloads')).length,1);
    assert.equal(paused.attempts.length,0);assert.equal(new Set(runs.map(r=>r.process_id)).size,4);
    const evidence={passed:true,synthetic:true,full_process_and_webview_restart:true,runs,crash_workspace:folder,pause_workspace:pauseFolder,crash_queries:crashQueries,pause_queries:queries};
    fs.writeFileSync(path.join(folder,'queue-restart-acceptance.json'),JSON.stringify(evidence,null,2));
    console.log('PASS real queue process restart: three zero results remain recorded; original scope/cursor retained; changed inputs and skip flags are not searched; new rows excluded; verified file reused; channel recovery retries current target only.');
    console.log('PASS user pause survives a full app/WebView2 crash; continuation resumes original scope; one native TXT per workspace; no upload/import/push writes.');
    console.log('Evidence:',path.join(folder,'queue-restart-acceptance.json'));
  }finally{for(const res of held)res.destroy();server.close();server.closeAllConnections();}
})().catch(error=>{console.error(error);process.exitCode=1;});
