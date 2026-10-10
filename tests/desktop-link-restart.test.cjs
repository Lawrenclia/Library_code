// Independent remote SA/front state survives the owned app and WebView2 exits.
const http=require('node:http'),fs=require('node:fs'),path=require('node:path');
const {spawn}=require('node:child_process'),{randomUUID}=require('node:crypto');
const {DatabaseSync}=require('node:sqlite'),assert=require('node:assert/strict');
const read=name=>fs.readFileSync(path.join(__dirname,'fixtures',name),'utf8');
const saFixture=read('compare.html'),frontFixture=read('library.html'),wosFixture=read('wos.html');
const root=path.resolve(__dirname,'../runtime/tauri-smoke');
const binary=process.env.DESKTOP_TEST_BINARY||path.resolve(__dirname,'../desktop/src-tauri/target/debug/library-workspace.exe');
const item='1244586319225556123';
const initial=()=>({sa:{saLzkId:'restart-link',gh:'001',itemId:'',matchCount:0,markStatus:'待处理',remark:'',reason:'通讯作者标记不一致',titleValue:'Synthetic paper'},saDoi:'10.1234/test',
 items:[{id:item,modelName:'期刊论文',metadata:{title:['Synthetic paper'],doi:['10.1234/test'],wosId:['WOS:000123456789012'],abstract:['Complete original abstract'],pages:['10-20'],
 author:[{id:'link-author-1',order:1,fullname:'Tester',scholarId:'scholar-001',correspondent:false,institutionOrderNums:['1']}],
 authorInstitution:[{id:'unit-sjtu',order:1,address:'Shanghai Jiao Tong University'}]}}],links:[],completed:[],reads:[]});
function snapshot(folder){const db=new DatabaseSync(path.join(folder,'workspace.sqlite3'),{readOnly:true});try{db.exec('PRAGMA busy_timeout=5000');return{tasks:db.prepare('SELECT data FROM tasks').all().map(r=>JSON.parse(r.data)),attempts:db.prepare('SELECT * FROM attempts ORDER BY created,rowid').all().map(r=>({...r,data:JSON.parse(r.data)}))};}finally{db.close();}}
(async()=>{
 let remote=initial(),phase='',boundary,absent=false;const replies=new Set(),runs=[];
 const sa=()=>saFixture.replace('姓名：测试员<br/>是否通讯作者：是','姓名：测试员<br/>工号：001<br/>是否通讯作者：是').replace('</script>',`
 const originalQuery=vm.getData.bind(vm);
 vm.getData=async function(){this.loading=true;const state=await(await fetch('/restart/state')).json();Object.assign(synthetic,state.sa);
  Object.assign(testConfig,{saClaim:'测试员(001)①',claim:'已认领',saDoi:state.saDoi,saWos:'WOS:000123456789012',libraryWos:'WOS:000123456789012',authorInfo:'署名：Tester<br/>工号：001<br/>是否通讯作者：否'});return originalQuery();};
 vm.handleEditItem=function(row){const modal=document.createElement('div');modal.className='el-message-box';modal.innerHTML='<p>请输入匹配条目的平台唯一号</p><input><button>确定</button>';document.body.appendChild(modal);
  modal.querySelector('button').onclick=async()=>{await fetch('/restart/link',{method:'POST',body:JSON.stringify({sa_id:row.saLzkId,item_id:modal.querySelector('input').value})});modal.remove();await vm.getData();};};
 statusModal.handleConfirm=async function(){await fetch('/restart/complete',{method:'POST',body:JSON.stringify({sa_id:this.currentRow.saLzkId,markStatus:'已处理',remark:this.editForm.remark})});this.dialogVisible=false;await vm.getData();};
 </script>`);
 const front=()=>frontFixture.replace('</script>',`
 const originalSubmit=vm.advanceSubmit.bind(vm);
 vm.advanceSubmit=async function(params){const state=await(await fetch('/restart/library')).json();testConfig.rows=state.items;return originalSubmit(params);};
 </script>`);
 const server=http.createServer((req,res)=>{
  if(['/restart/state','/restart/library'].includes(req.url)){if(req.url==='/restart/library')remote.reads.push({phase});res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify(remote));return;}
  if(['/restart/link','/restart/complete'].includes(req.url)&&req.method==='POST'){
   let body='';req.on('data',b=>body+=b);req.on('end',()=>{const data=JSON.parse(body);
    assert.equal(data.sa_id,'restart-link');
    if(req.url==='/restart/link'){
     assert.equal(data.item_id,item);remote.links.push(structuredClone(data));if(!absent)Object.assign(remote.sa,{itemId:item,matchCount:1});
     if(phase.endsWith('-start')){replies.add(res);res.once('close',()=>replies.delete(res));boundary?.();return;}
    }else{remote.completed.push(data);Object.assign(remote.sa,{markStatus:data.markStatus,remark:data.remark});}
    res.writeHead(200);res.end();});return;
  }
  const html=req.url==='/advancedSearch'?front():req.url.startsWith('/wos/')?wosFixture:sa();
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
  let timer;const timed=new Promise((_,reject)=>timer=setTimeout(()=>reject(Error(`Link ${next} timed out: ${output}`)),150000));
  try{
   if(crash){
    await Promise.race([accepted,exited.then(r=>{throw Error(`Exited before link receipt: ${JSON.stringify(r)} ${output}`);}),timed]);
    const local=snapshot(folder);assert.equal(local.tasks.length,1);assert.equal(local.tasks[0].running,true);assert.equal(local.attempts.length,1);
    assert.equal(local.attempts[0].action,'link');assert.equal(local.attempts[0].state,'intent');assert.equal(remote.links.length,1);
    assert.deepEqual(local.attempts[0].data.payload.selected_item,remote.items[0]);
    fs.writeFileSync(path.join(folder,`boundary-${next}.json`),JSON.stringify({process_id:child.pid,local,remote},null,2));
    await terminate(child,exited);assert.equal(fs.existsSync(path.join(folder,`restart-${next}.json`)),false);runs.push({phase:next,process_id:child.pid,terminated:true});
   }else{
    const exit=await Promise.race([exited,timed]);const report=JSON.parse(fs.readFileSync(path.join(folder,`restart-${next}.json`),'utf8'));
    assert.equal(exit.code,0,JSON.stringify(report.failure));assert.equal(report.passed,true,JSON.stringify(report.failure));assert.equal(report.process_id,child.pid);runs.push({phase:next,process_id:child.pid,passed:true});
   }return folder;
  }finally{clearTimeout(timer);await terminate(child,exited);boundary=null;for(const reply of replies)reply.destroy();}
 }
 function unknown(folder){const s=snapshot(folder);assert.equal(s.tasks[0].stage,'unknown');assert.equal(s.attempts[0].state,'unknown');assert.equal(s.tasks[0].record.done,false);assert.equal(s.tasks[0].evidence.filter(e=>e.kind==='sa_link_verified').length,0);assert.equal(remote.links.length,1);assert.equal(remote.completed.length,0);return s;}
 function completed(folder,start){
  const s=snapshot(folder),b=JSON.parse(fs.readFileSync(path.join(folder,`boundary-${start}.json`),'utf8'));
  assert.equal(s.tasks[0].stage,'completed');assert.equal(s.tasks[0].record.done,true);assert.equal(s.tasks[0].record.matches,0);assert.equal(s.tasks[0].issue_reviews.length,1);
  assert.equal(s.attempts[0].state,'verified');assert.deepEqual(s.attempts[0].data.payload,b.local.attempts[0].data.payload);
  const proofs=s.tasks[0].evidence.filter(e=>e.kind==='sa_link_verified');assert.equal(proofs.length,2,'Recovery and explicit existing-link readback each retain complete context');
  const p=JSON.parse(proofs[0].text);assert.deepEqual(p.payload,b.local.attempts[0].data.payload);assert.deepEqual(p.payload.selected_item,remote.items[0]);
  assert.equal(remote.links.length,1);assert.equal(remote.completed.length,1);assert.ok(remote.reads.some(r=>r.phase===phase),'Restart must query the actual front end anew');return s;
 }
 try{
  const id=randomUUID(),folder=await run(id,'link-start',true),committed=structuredClone(remote);
  remote.sa.remark='Changed remote remark';await run(id,'link-sa-reject');unknown(folder);
  remote=structuredClone(committed);remote.saDoi='10.1234/changed';await run(id,'link-source-reject');unknown(folder);
  remote=structuredClone(committed);remote.items[0].metadata.abstract=['Changed remote publication'];await run(id,'link-metadata-reject');unknown(folder);
  remote=structuredClone(committed);remote.items=[];await run(id,'link-missing-reject');unknown(folder);
  remote=structuredClone(committed);remote.sa.gh='002';await run(id,'link-staff-reject');unknown(folder);
  remote=structuredClone(committed);await run(id,'link-resume');completed(folder,'link-start');const successful=structuredClone(remote);
  remote=initial();absent=true;const absentFolder=await run(randomUUID(),'link-absent-start',true);await run(path.basename(absentFolder),'link-absent-resume');unknown(absentFolder);const absentRemote=structuredClone(remote);
  remote=initial();absent=false;const pushedFolder=await run(randomUUID(),'link-pushed-start',true);await run(path.basename(pushedFolder),'link-pushed-resume');completed(pushedFolder,'link-pushed-start');
  assert.equal(snapshot(pushedFolder).attempts[0].data.payload.previous_stage,'pushed');
  assert.equal(new Set(runs.map(r=>r.process_id)).size,11,'Every restart uses a different actual application process');
  fs.writeFileSync(path.join(folder,'link-restart-acceptance.json'),JSON.stringify({passed:true,synthetic:true,full_process_and_webview_restart:true,runs,link_workspace:folder,absent_workspace:absentFolder,pushed_workspace:pushedFolder,successful_remote:successful,absent_remote:absentRemote,pushed_remote:remote},null,2));
  console.log('Native association restart PASS: original zero-match roster/complete SA/review/front item -> link received once -> owned app/WebView2 killed -> new process rejects writes -> fresh complete front query and SA readback -> original checkpoint restored atomically -> independent reason review -> SA completed once.');
  console.log('Changed SA source/row/staff, changed publication, missing candidate and actually uncommitted link remain Unknown with exact original intent and no repeat write. Corrected-existing and seeded pushed checkpoints covered; upload/import/push is not exercised in this test.');
  console.log('Evidence:',path.join(folder,'link-restart-acceptance.json'));
 }finally{for(const reply of replies)reply.destroy();server.close();server.closeAllConnections();}
})().catch(error=>{console.error(error);process.exitCode=1;});
