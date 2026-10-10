// Independent remote state survives termination of the actual app and WebView2.
const http=require('node:http'),fs=require('node:fs'),path=require('node:path');
const {spawn}=require('node:child_process'),{randomUUID}=require('node:crypto');
const {DatabaseSync}=require('node:sqlite'),assert=require('node:assert/strict');
const fixture=name=>fs.readFileSync(path.join(__dirname,'fixtures',name),'utf8');
const root=path.resolve(__dirname,'../runtime/tauri-smoke');
const binary=process.env.DESKTOP_TEST_BINARY||path.resolve(__dirname,'../desktop/src-tauri/target/debug/library-workspace.exe');
const initial=()=>[{id:'alias-old',scholarId:'scholar-001',nameAlias:'Tester',defaultNameCn:1,defaultNameEn:0,isSet:false}];
function snapshot(folder){const db=new DatabaseSync(path.join(folder,'workspace.sqlite3'),{readOnly:true});try{db.exec('PRAGMA busy_timeout=5000');return{tasks:db.prepare('SELECT data FROM tasks').all().map(r=>JSON.parse(r.data)),attempts:db.prepare('SELECT * FROM attempts ORDER BY created,rowid').all().map(r=>({...r,data:JSON.parse(r.data)}))};}finally{db.close();}}
(async()=>{
 let remote,phase='',boundary,absent=false;const replies=new Set(),runs=[];
 const reset=()=>remote={aliases:initial(),saves:[],scholar_changed:false,sa_changed:false,reads:0};reset();
 const scholar=()=>fixture('scholar.html').replaceAll("'00001'","'001'").replace('</script>',`
  modal.getData=async function(){this.loading=true;this.data=await(await fetch('/restart/aliases')).json();this.loading=false;};
  modal.onSubmit=async function(row){aliasWrites++;this.loading=true;await fetch('/restart/alias-save',{method:'POST',body:JSON.stringify(row)});await this.getData();};
  const originalGetData=vm.getData.bind(vm);
  vm.getData=async function(){const person=await(await fetch('/restart/scholar')).json();testConfig.people=[person];return originalGetData();};
 </script>`);
 const sa=()=>fixture('compare.html').replace('</script>',`
  Object.assign(synthetic,{saLzkId:'restart-alias',gh:'001',matchCount:1,itemId:'item-alias',reason:'作者不一致',remark:${JSON.stringify(remote.sa_changed?'Changed remote remark':'')}});
  Object.assign(testConfig,{saClaim:'测试员(001)①',authorInfo:'工号：001<br/>署名：Demo, X',saDoi:'10.1234/test'});
 </script>`);
 const server=http.createServer((req,res)=>{
  if(req.url==='/restart/aliases'){remote.reads++;res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify(remote.aliases));return;}
  if(req.url==='/restart/scholar'){res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify({id:'scholar-001',wno:'001',nameCn:'测试员',nameEn:remote.scholar_changed?'Changed':'Tester'}));return;}
  if(req.url==='/restart/alias-save'&&req.method==='POST'){
   let body='';req.on('data',b=>body+=b);req.on('end',()=>{const row=JSON.parse(body);remote.saves.push(structuredClone(row));
    if(!absent)remote.aliases.push({...row,id:'alias-new',isSet:false});
    if(phase.endsWith('-start')){replies.add(res);res.once('close',()=>replies.delete(res));boundary?.();return;}
    res.writeHead(200);res.end();});return;
  }
  const html=req.url==='/native/scholar'?scholar():sa();
  for(const m of html.matchAll(/<script>([\s\S]*?)<\/script>/g))new(require('node:vm').Script)(m[1]);
  res.writeHead(200,{'Content-Type':'text/html; charset=utf-8'});res.end(html);
 });
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const origin=`http://127.0.0.1:${server.address().port}/`;
 async function terminate(child,exited){
  if(child.exitCode!==null||child.signalCode!==null)return;
  const killer=spawn('taskkill',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'pipe'});let output='';killer.stderr.on('data',b=>output+=b);
  assert.equal(await new Promise((resolve,reject)=>{killer.once('exit',resolve);killer.once('error',reject);}),0,output);await exited;
 }
 async function run(id,next,crash=false){
  phase=next;const accepted=new Promise(resolve=>boundary=resolve),folder=path.join(root,id);
  const env={...process.env,DESKTOP_SMOKE_ORIGIN:origin,DESKTOP_SMOKE_RESTART_ID:id,DESKTOP_SMOKE_RESTART_PHASE:next};delete env.DESKTOP_SMOKE_LOSE_UPLOAD_RESPONSE;
  const child=spawn(binary,[],{env,windowsHide:true,stdio:'pipe'});let output='';
  const exited=new Promise((resolve,reject)=>{child.once('exit',(code,signal)=>resolve({code,signal}));child.once('error',reject);});
  for(const stream of [child.stdout,child.stderr])stream.on('data',b=>{output+=b;process.stdout.write(b);});
  let timer;const timed=new Promise((_,reject)=>timer=setTimeout(()=>reject(Error(`Alias ${next} timed out: ${output}`)),150000));
  try{
   if(crash){
    await Promise.race([accepted,exited.then(r=>{throw Error(`Exited before accepted alias: ${JSON.stringify(r)} ${output}`);}),timed]);
    const local=snapshot(folder);assert.equal(local.tasks.length,1);assert.equal(local.tasks[0].running,true);assert.equal(local.attempts.length,1);
    assert.equal(local.attempts[0].action,'add_alias');assert.equal(local.attempts[0].state,'intent');assert.equal(remote.saves.length,1);
    fs.writeFileSync(path.join(folder,`boundary-${next}.json`),JSON.stringify({process_id:child.pid,local,remote},null,2));
    await terminate(child,exited);assert.equal(fs.existsSync(path.join(folder,`restart-${next}.json`)),false);
    runs.push({phase:next,process_id:child.pid,terminated:true});
   }else{
    const exit=await Promise.race([exited,timed]);const report=JSON.parse(fs.readFileSync(path.join(folder,`restart-${next}.json`),'utf8'));
    assert.equal(exit.code,0,JSON.stringify(report.failure));assert.equal(report.passed,true,JSON.stringify(report.failure));assert.equal(report.process_id,child.pid);
    runs.push({phase:next,process_id:child.pid,passed:true});
   }
   return folder;
  }finally{clearTimeout(timer);await terminate(child,exited);boundary=null;for(const reply of replies)reply.destroy();}
 }
 function unknown(folder){const s=snapshot(folder);assert.equal(s.tasks[0].stage,'unknown');assert.equal(s.attempts[0].state,'unknown');assert.equal(s.tasks[0].record.done,false);assert.equal(s.tasks[0].evidence.filter(e=>e.kind==='alias_verified').length,0);assert.equal(remote.saves.length,1);return s;}
 try{
  const id=randomUUID(),folder=await run(id,'alias-start',true),committed=structuredClone(remote.aliases);
  remote.aliases[0].defaultNameCn=0;await run(id,'alias-list-reject');unknown(folder);
  remote.aliases=structuredClone(committed);remote.scholar_changed=true;await run(id,'alias-scholar-reject');unknown(folder);
  remote.scholar_changed=false;remote.sa_changed=true;await run(id,'alias-sa-reject');unknown(folder);
  remote.sa_changed=false;await run(id,'alias-resume');
  const final=snapshot(folder),original=JSON.parse(fs.readFileSync(path.join(folder,'boundary-alias-start.json'),'utf8')).local;
  assert.equal(final.tasks[0].stage,'pending');assert.equal(final.tasks[0].record.done,false);assert.equal(final.tasks[0].issue_reviews.length,0);
  assert.equal(final.attempts.length,2);assert.equal(final.attempts[0].state,'verified');assert.deepEqual(final.attempts[0].data.payload,original.attempts[0].data.payload);
  assert.equal(final.attempts[1].data.response.already_present,true);assert.equal(remote.saves.length,1);assert.deepEqual(remote.aliases,committed);
  const proofs=final.tasks[0].evidence.filter(e=>e.kind==='alias_verified');assert.equal(proofs.length,2);
  const proof=JSON.parse(proofs[0].text);assert.deepEqual(proof.payload,original.attempts[0].data.payload);assert.equal(proof.result.scholar.wno,'001');
  assert.equal(proof.result.aliases.length,2);assert.equal(proof.result.aliases.find(a=>a.id==='alias-old').defaultNameCn,1);
  const successful=structuredClone(remote);reset();absent=true;const absentFolder=await run(randomUUID(),'alias-absent-start',true);
  await run(path.basename(absentFolder),'alias-absent-resume');unknown(absentFolder);assert.deepEqual(remote.aliases,initial());
  assert.equal(new Set(runs.map(r=>r.process_id)).size,7,'Every recovery phase uses a different real process');
  fs.writeFileSync(path.join(folder,'alias-restart-acceptance.json'),JSON.stringify({passed:true,synthetic:true,full_process_and_webview_restart:true,runs,alias_workspace:folder,absent_workspace:absentFolder,successful_remote:successful,absent_remote:remote},null,2));
  console.log('Native alias restart PASS: original full list/source/identity -> server received once -> owned app/WebView2 terminated -> new process refuses writes -> exact original intent and full readback -> original stage atomically restored; existing alias recheck makes no additional save and SA remains pending.');
  console.log('Unknown safeguards PASS: changed original default name, scholar identity, SA row or actual absence retain original intent, no verified source and no repeat save.');
  console.log('Evidence:',path.join(folder,'alias-restart-acceptance.json'));
 }finally{for(const reply of replies)reply.destroy();server.close();server.closeAllConnections();}
})().catch(error=>{console.error(error);process.exitCode=1;});
