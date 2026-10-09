// External server retains original metadata/claim state across actual app exits.
const http=require('node:http'),fs=require('node:fs'),path=require('node:path');
const {spawn}=require('node:child_process'),{randomUUID}=require('node:crypto');
const {DatabaseSync}=require('node:sqlite'),assert=require('node:assert/strict');
const fixture=fs.readFileSync(path.join(__dirname,'fixtures/compare.html'),'utf8');
const root=path.resolve(__dirname,'../runtime/tauri-smoke');
const binary=process.env.DESKTOP_TEST_BINARY||path.resolve(__dirname,'../desktop/src-tauri/target/debug/library-workspace.exe');
const initial=()=>({metadata:{title:['Synthetic paper'],doi:['10.1234/test'],abstract:['Complete original abstract'],pages:['10-20'],
 author:[{id:'claim-author-1',order:1,fullname:'Demo',scholarId:null,correspondent:false,institutionOrderNums:['1']},
 {id:'claim-author-2',order:2,fullname:'Other',scholarId:'scholar-other',correspondent:true,institutionOrderNums:['2']}],
 authorInstitution:[{id:'unit-sjtu',order:1,address:'Shanghai Jiao Tong University'},{id:'unit-other',order:2,address:'Synthetic University'}]},
 relations:{'2':[{scholarId:'scholar-other',status:6}]},person:{id:'scholar-001',wno:'001',nameCn:'测试员',nameEn:'Tester',aliases:[{nameAlias:'Demo'}]},
 sa:{markStatus:'待处理',remark:''},claims:[],completed:[]});
function snapshot(folder){const db=new DatabaseSync(path.join(folder,'workspace.sqlite3'),{readOnly:true});try{db.exec('PRAGMA busy_timeout=5000');return{tasks:db.prepare('SELECT data FROM tasks').all().map(r=>JSON.parse(r.data)),attempts:db.prepare('SELECT * FROM attempts ORDER BY created,rowid').all().map(r=>({...r,data:JSON.parse(r.data)}))};}finally{db.close();}}
(async()=>{
 let remote=initial(),phase='',boundary,absent=false;const replies=new Set(),runs=[];
 const html=()=>fixture.replace('姓名：测试员<br/>是否通讯作者：是','姓名：测试员<br/>工号：001<br/>是否通讯作者：是').replace('</script>',`
  Object.assign(synthetic,{saLzkId:'restart-claim',gh:'001',matchCount:1,itemId:'item-claim',reason:'作者不一致'});
  const originalQuery=vm.getData.bind(vm);
  vm.getData=async function(){this.loading=true;const state=await(await fetch('/restart/state')).json();
   Object.assign(synthetic,state.sa);synthetic.claimStatus=state.metadata.author[0].scholarId==='scholar-001';
   Object.assign(testConfig,{saClaim:'测试员(001)①',claim:synthetic.claimStatus?'已认领':'未认领',saDoi:'10.1234/test',authorInfo:'署名：Demo<br/>工号：001<br/>是否第一作者：否<br/>是否通讯作者：否'});return originalQuery();};
  claimWindow.getItemDetail=async function(){this.loading=true;const state=await(await fetch('/restart/state')).json();
   this.tableData={id:this.ids,metadata:copy(state.metadata),itemAuthorRelationVOs:copy(state.relations)};this.drawer=true;this.loading=false;};
  claimWindow.handleClaim=async function(author){writeCount++;claimWrites++;this.loading=true;
   await fetch('/restart/claim',{method:'POST',body:JSON.stringify({item_id:this.ids,author_id:author.id,order:author.order,person:author.data})});await this.getItemDetail();};
  const originalPeople=people.getScholarData.bind(people);
  people.getScholarData=async function(){const state=await(await fetch('/restart/state')).json();testConfig.people=[state.person];return originalPeople();};
  statusModal.handleConfirm=async function(){await fetch('/restart/complete',{method:'POST',body:JSON.stringify({sa_id:this.currentRow.saLzkId,markStatus:'已处理',remark:this.editForm.remark})});this.dialogVisible=false;await vm.getData();};
 </script>`);
 const server=http.createServer((req,res)=>{
  if(req.url==='/restart/state'){res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify(remote));return;}
  if(['/restart/claim','/restart/complete'].includes(req.url)&&req.method==='POST'){
   let body='';req.on('data',b=>body+=b);req.on('end',()=>{const data=JSON.parse(body);
    if(req.url==='/restart/claim'){
     assert.equal(data.item_id,'item-claim');assert.equal(data.author_id,'claim-author-1');assert.equal(data.person.wno,'001');remote.claims.push(structuredClone(data));
     if(!absent)remote.metadata.author.find(a=>a.id===data.author_id).scholarId=data.person.scholarId;
     if(phase.endsWith('-start')){replies.add(res);res.once('close',()=>replies.delete(res));boundary?.();return;}
    }else{remote.completed.push(data);Object.assign(remote.sa,{markStatus:data.markStatus,remark:data.remark});}
    res.writeHead(200);res.end();});return;
  }
  const page=html();for(const m of page.matchAll(/<script>([\s\S]*?)<\/script>/g))new(require('node:vm').Script)(m[1]);
  res.writeHead(200,{'Content-Type':'text/html; charset=utf-8'});res.end(page);
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
  let timer;const timed=new Promise((_,reject)=>timer=setTimeout(()=>reject(Error(`Claim ${next} timed out: ${output}`)),150000));
  try{
   if(crash){
    await Promise.race([accepted,exited.then(r=>{throw Error(`Exited before accepted claim: ${JSON.stringify(r)} ${output}`);}),timed]);
    const local=snapshot(folder);assert.equal(local.tasks.length,1);assert.equal(local.tasks[0].running,true);assert.equal(local.attempts.length,1);
    assert.equal(local.attempts[0].action,'submit_claim');assert.equal(local.attempts[0].state,'intent');assert.equal(remote.claims.length,1);
    fs.writeFileSync(path.join(folder,`boundary-${next}.json`),JSON.stringify({process_id:child.pid,local,remote},null,2));
    await terminate(child,exited);assert.equal(fs.existsSync(path.join(folder,`restart-${next}.json`)),false);runs.push({phase:next,process_id:child.pid,terminated:true});
   }else{
    const exit=await Promise.race([exited,timed]);const report=JSON.parse(fs.readFileSync(path.join(folder,`restart-${next}.json`),'utf8'));
    assert.equal(exit.code,0,JSON.stringify(report.failure));assert.equal(report.passed,true,JSON.stringify(report.failure));assert.equal(report.process_id,child.pid);runs.push({phase:next,process_id:child.pid,passed:true});
   }return folder;
  }finally{clearTimeout(timer);await terminate(child,exited);boundary=null;for(const reply of replies)reply.destroy();}
 }
 function unknown(folder){const s=snapshot(folder);assert.equal(s.tasks[0].stage,'unknown');assert.equal(s.attempts[0].state,'unknown');assert.equal(s.tasks[0].record.done,false);assert.equal(s.tasks[0].evidence.filter(e=>e.kind==='claim_verified').length,0);assert.equal(remote.claims.length,1);assert.equal(remote.completed.length,0);return s;}
 try{
  const id=randomUUID(),folder=await run(id,'claim-start',true),committed=structuredClone(remote);
  remote.metadata.author[0].id='replacement';await run(id,'claim-id-reject');unknown(folder);
  remote=structuredClone(committed);remote.metadata.abstract=['Unrelated remote change'];await run(id,'claim-metadata-reject');unknown(folder);
  remote=structuredClone(committed);remote.person.nameCn='Changed';await run(id,'claim-person-reject');unknown(folder);
  remote=structuredClone(committed);remote.sa.remark='Changed remote remark';await run(id,'claim-sa-reject');unknown(folder);
  remote=structuredClone(committed);await run(id,'claim-resume');
  const final=snapshot(folder),original=JSON.parse(fs.readFileSync(path.join(folder,'boundary-claim-start.json'),'utf8')).local;
  assert.equal(final.tasks[0].stage,'completed');assert.equal(final.tasks[0].record.done,true);assert.equal(final.tasks[0].issue_reviews.length,2);
  assert.equal(final.attempts[0].state,'verified');assert.deepEqual(final.attempts[0].data.payload,original.attempts[0].data.payload);
  assert.equal(remote.claims.length,1);assert.equal(remote.completed.length,1);assert.deepEqual(remote.metadata,committed.metadata);
  const proofs=final.tasks[0].evidence.filter(e=>e.kind==='claim_verified');assert.equal(proofs.length,1);const proof=JSON.parse(proofs[0].text);
  assert.deepEqual(proof.payload,original.attempts[0].data.payload);assert.deepEqual(proof.result.metadata,remote.metadata);assert.equal(proof.result.author_id,'claim-author-1');assert.equal(proof.result.staff_id,'001');
  const successful=structuredClone(remote);remote=initial();absent=true;const absentFolder=await run(randomUUID(),'claim-absent-start',true);
  await run(path.basename(absentFolder),'claim-absent-resume');unknown(absentFolder);assert.deepEqual(remote.metadata,initial().metadata);
  assert.equal(new Set(runs.map(r=>r.process_id)).size,8,'Every restart uses a different actual process');
  fs.writeFileSync(path.join(folder,'claim-restart-acceptance.json'),JSON.stringify({passed:true,synthetic:true,full_process_and_webview_restart:true,runs,claim_workspace:folder,absent_workspace:absentFolder,successful_remote:successful,absent_remote:remote},null,2));
  console.log('Native claim restart PASS: exact original author ID/full metadata/SA/staff -> server accepted once -> owned app and WebView2 killed -> new process refuses writes -> fresh full staff query and complete readback -> atomic Claimed checkpoint -> independent issue review -> SA completed once.');
  console.log('Unknown safeguards PASS: replaced author ID with same name/order, unrelated metadata, changed scholar identity or SA, and actual uncommitted claim retain exact original intent, no verified conclusion and no repeat claim/SA completion.');
  console.log('Evidence:',path.join(folder,'claim-restart-acceptance.json'));
 }finally{for(const reply of replies)reply.destroy();server.close();server.closeAllConnections();}
})().catch(error=>{console.error(error);process.exitCode=1;});
