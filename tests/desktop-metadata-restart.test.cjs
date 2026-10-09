// Actual app/WebView2 termination. The loopback server owns remote metadata
// across processes; production editor adapters perform the read/write steps.
const http=require('node:http'),fs=require('node:fs'),path=require('node:path');
const {spawn}=require('node:child_process'),{randomUUID}=require('node:crypto');
const {DatabaseSync}=require('node:sqlite'),assert=require('node:assert/strict');
const fixture=name=>fs.readFileSync(path.join(__dirname,'fixtures',name),'utf8');
const root=path.resolve(__dirname,'../runtime/tauri-smoke');
const binary=process.env.DESKTOP_TEST_BINARY||path.resolve(__dirname,'../desktop/src-tauri/target/debug/library-workspace.exe');
const initial=()=>({id:'item-metadata',modelId:'model-fixture',modelName:'期刊论文',datasetIds:['dataset-fixture'],dataSources:['WOS'],fullTexts:[{id:'source-attachment',name:'original.pdf'}],
 metadata:{title:['Synthetic paper'],doi:['10.1234/test'],pages:['10-20'],abstract:['Complete original abstract'],volume:['12'],issue:['2'],issued:['2026-01-01'],author:[
  {id:'native-author-1',fullname:'Other',order:1,scholarId:'',institutionOrderNums:'1',correspondent:false,commonFirst:false,email:'other@example.invalid'},
  {id:'native-author-2',fullname:'Tester',order:2,scholarId:'scholar-001',institutionOrderNums:'1,2',correspondent:false,commonFirst:false,ownFirst:false,email:'tester@example.invalid'}],
 authorInstitution:[{id:'unit-other',order:1,address:'Synthetic University',topInstitutionId:'synthetic'},
  {id:'unit-sjtu',order:2,address:'Shanghai Jiao Tong University',topInstitutionId:'1244586319225556993'}]}});
function snapshot(folder){const db=new DatabaseSync(path.join(folder,'workspace.sqlite3'),{readOnly:true});try{db.exec('PRAGMA busy_timeout=5000');return {tasks:db.prepare('SELECT data FROM tasks').all().map(r=>JSON.parse(r.data)),
 attempts:db.prepare('SELECT * FROM attempts ORDER BY created,rowid').all().map(r=>({...r,data:JSON.parse(r.data)}))};}finally{db.close();}}
function normal(form){const copy=structuredClone(form);for(const author of copy.metadata.author)if(Array.isArray(author.institutionOrderNums))author.institutionOrderNums=author.institutionOrderNums.join(',');return copy;}
(async()=>{
 let scenario='author',remote,phase='',boundary;const replies=new Set(),runs=[];
 const reset=kind=>{scenario=kind;remote={form:initial(),saves:[],completed:[],reads:0,markStatus:'待处理',remark:''};};reset('author');
 const saPage=()=>{
  const editor=fixture('metadata-native.js')
   .replace('const pristine=copy(saved);','saved=copy(window.remoteMetadata);const pristine=copy(saved);')
   .replace('this.ruleForm=copy(saved);',"saved=await(await fetch('/restart/metadata')).json();this.ruleForm=copy(saved);")
   .replace("'/native/metadata-save'","'/restart/metadata-save'")
   .replace("['smoke-metadata','smoke-order'].includes(row.saLzkId)","row.saLzkId==='restart-metadata'")
   .replace("row.saLzkId==='smoke-order'","testConfig.unitCase")
   .replace('const author=saved.metadata.author.find','const author=window.remoteMetadata.metadata.author.find')
   .replace('this.getKmsTopInstitution(saved)','this.getKmsTopInstitution(window.remoteMetadata)')
   .replace("info.compareLeftValue='工号：001<br/>是否第一作者：是<br/>是否通讯作者：否';","info.compareLeftValue='工号：001<br/>是否第一作者：否<br/>是否通讯作者：否';");
  const reason=scenario==='unit'?'交大是否第一单位不一致':scenario==='role'?'通讯作者标记不一致':'第一作者标记不一致';
  return fixture('compare.html').replace('姓名：测试员<br/>是否通讯作者：是',`工号：001<br/>是否第一作者：${['unit','role'].includes(scenario)||remote.sa_changed?'否':'是'}<br/>是否通讯作者：${scenario==='role'?'是':'否'}`)
   .replace('</script>',`
    window.remoteMetadata=${JSON.stringify(remote.form)};
    Object.assign(synthetic,{saLzkId:'restart-metadata',gh:'001',matchCount:1,itemId:'item-metadata',reason:${JSON.stringify(reason)}});
    Object.assign(testConfig,{unitCase:${scenario==='unit'},claim:'已认领',saClaim:'测试员(001)①',saDoi:'10.1234/test'});
    const originalQuery=vm.getData.bind(vm);
    vm.getData=async function(){this.loading=true;window.remoteMetadata=await(await fetch('/restart/metadata')).json();
      const state=await(await fetch('/restart/sa')).json();synthetic.markStatus=state.markStatus;synthetic.remark=state.remark;return originalQuery();};
    statusModal.handleConfirm=async function(){await fetch('/restart/complete',{method:'POST',body:JSON.stringify({sa_id:this.currentRow.saLzkId,markStatus:this.currentRow.markStatus==='待处理'?'已处理':'待处理',remark:this.editForm.remark})});this.dialogVisible=false;await vm.getData();};
    ${editor}
   </script>`);
 };
 const scholar=fixture('scholar.html').replaceAll("'00001'","'001'");
 const server=http.createServer((req,res)=>{
  if(req.url==='/restart/metadata'){remote.reads++;res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify(remote.form));return;}
  if(req.url==='/restart/sa'){res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify({markStatus:remote.markStatus,remark:remote.remark}));return;}
  if(req.url==='/restart/metadata-save'&&req.method==='POST'){
   let body='';req.on('data',b=>body+=b);req.on('end',()=>{const form=JSON.parse(body).form;remote.saves.push(structuredClone(form));
    if(scenario!=='absent')remote.form=structuredClone(form);
    if(phase.endsWith('-start')){replies.add(res);res.once('close',()=>replies.delete(res));boundary?.resolve();return;}
    res.writeHead(200);res.end();});return;
  }
  if(req.url==='/restart/complete'&&req.method==='POST'){
   let body='';req.on('data',b=>body+=b);req.on('end',()=>{const state=JSON.parse(body);remote.completed.push(state);remote.markStatus=state.markStatus;remote.remark=state.remark;res.writeHead(200);res.end();});return;
  }
  const html=req.url==='/native/scholar'?scholar:saPage();
  for(const match of html.matchAll(/<script>([\s\S]*?)<\/script>/g))new(require('node:vm').Script)(match[1]);
  res.writeHead(200,{'Content-Type':'text/html; charset=utf-8'});res.end(html);
 });
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 const origin=`http://127.0.0.1:${server.address().port}/`;
 async function terminate(child,exited){
  if(child.exitCode!==null||child.signalCode!==null)return;
  const killer=spawn('taskkill',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'pipe'});let output='';killer.stderr.on('data',b=>output+=b);
  const code=await new Promise((resolve,reject)=>{killer.once('exit',resolve);killer.once('error',reject);});assert.equal(code,0,output);await exited;
 }
 async function run(id,next,crash=false){
  phase=next;let resolveBoundary;const accepted=new Promise(resolve=>resolveBoundary=resolve);boundary={resolve:resolveBoundary};
  const folder=path.join(root,id),env={...process.env,DESKTOP_SMOKE_ORIGIN:origin,DESKTOP_SMOKE_RESTART_ID:id,DESKTOP_SMOKE_RESTART_PHASE:next};
  delete env.DESKTOP_SMOKE_LOSE_UPLOAD_RESPONSE;
  const child=spawn(binary,[],{env,windowsHide:true,stdio:'pipe'});let output='';
  const exited=new Promise((resolve,reject)=>{child.once('exit',(code,signal)=>resolve({code,signal}));child.once('error',reject);});
  for(const stream of [child.stdout,child.stderr])stream.on('data',b=>{output+=b;process.stdout.write(b);});
  let timeout;const timed=new Promise((_,reject)=>{timeout=setTimeout(()=>reject(Error(`Metadata ${next} timed out: ${output}`)),150000);});
  try{
   if(crash){
    await Promise.race([accepted,exited.then(r=>{throw Error(`Exited before accepted edit: ${JSON.stringify(r)} ${output}`);}),timed]);
    const before=snapshot(folder);assert.equal(before.tasks.length,1);assert.equal(before.tasks[0].running,true);
    assert.equal(before.attempts.length,1);assert.equal(before.attempts[0].action,'save_metadata');assert.equal(before.attempts[0].state,'intent');
    assert.equal(before.tasks[0].issue_reviews.length,0);assert.equal(remote.saves.length,1);
    fs.writeFileSync(path.join(folder,`boundary-${next}.json`),JSON.stringify({process_id:child.pid,local:before,remote},null,2));
    await terminate(child,exited);assert.equal(fs.existsSync(path.join(folder,`restart-${next}.json`)),false,'App must stop before local result is saved');
    runs.push({phase:next,process_id:child.pid,terminated:true});
   }else{
    const exit=await Promise.race([exited,timed]);const report=JSON.parse(fs.readFileSync(path.join(folder,`restart-${next}.json`),'utf8'));
    assert.equal(exit.code,0,JSON.stringify(report.failure));assert.equal(report.passed,true,JSON.stringify(report.failure));assert.equal(report.process_id,child.pid);
    runs.push({phase:next,process_id:child.pid,passed:true});
   }
   return folder;
  }finally{clearTimeout(timeout);await terminate(child,exited);boundary=null;for(const response of replies)response.destroy();}
 }
 const good=folder=>{
  const final=snapshot(folder),original=JSON.parse(fs.readFileSync(path.join(folder,`boundary-metadata-${scenario}-start.json`),'utf8')).local;
  const attempt=final.attempts.find(a=>a.action==='save_metadata');assert.equal(attempt.state,'verified');assert.deepEqual(attempt.data.payload,original.attempts[0].data.payload);
  assert.equal(remote.saves.length,1);assert.equal(remote.completed.length,1);assert.equal(remote.completed[0].markStatus,'已处理');
  assert.equal(final.tasks[0].stage,'completed');assert.equal(final.tasks[0].record.done,true);assert.equal(final.tasks[0].issue_reviews.length,1);
  const evidence=final.tasks[0].evidence.filter(e=>e.kind==='metadata_verified');assert.equal(evidence.length,1);
  const verified=JSON.parse(evidence[0].text);assert.deepEqual(normal(verified.result.after),normal(remote.saves[0]));
  const expected=attempt.data.payload.expected_form||structuredClone(attempt.data.payload.expected_snapshot.form);
  if(!attempt.data.payload.expected_form)expected.metadata.author[attempt.data.payload.author_index].correspondent=attempt.data.payload.value;
  assert.deepEqual(normal(verified.result.after),normal(expected));assert.deepEqual(verified.result.before,attempt.data.payload.expected_snapshot.form);
  assert.equal(verified.payload.author_id,'native-author-2');assert.equal(verified.payload.staff_id,'001');
  return final;
 };
 try{
  const authorId=randomUUID();await run(authorId,'metadata-author-start',true);const committed=structuredClone(remote.form);
  remote.form.metadata.abstract=['An unrelated remote change'];const authorFolder=await run(authorId,'metadata-author-reject');
  const rejected=snapshot(authorFolder);assert.equal(rejected.tasks[0].stage,'unknown');assert.equal(rejected.attempts[0].state,'unknown');assert.equal(remote.saves.length,1);assert.equal(remote.completed.length,0);
  remote.form=committed;remote.sa_changed=true;await run(authorId,'metadata-author-sa-reject');
  const changedSa=snapshot(authorFolder);assert.equal(changedSa.tasks[0].stage,'unknown');assert.equal(changedSa.attempts[0].state,'unknown');
  assert.equal(changedSa.tasks[0].issue_reviews.length,0);assert.equal(changedSa.tasks[0].evidence.filter(e=>e.kind==='metadata_verified').length,0);
  assert.equal(remote.saves.length,1);assert.equal(remote.completed.length,0);
  remote.sa_changed=false;await run(authorId,'metadata-author-resume');good(authorFolder);
  assert.deepEqual(remote.form.metadata.author.map(a=>[a.id,a.order,a.commonFirst]),[['native-author-2',1,false],['native-author-1',2,false]]);
  const authorRemote=structuredClone(remote);
  reset('unit');const unitId=randomUUID();await run(unitId,'metadata-unit-start',true);const unitFolder=await run(unitId,'metadata-unit-resume');good(unitFolder);
  assert.deepEqual(remote.form.metadata.authorInstitution.map(u=>[u.id,u.order]),[['unit-sjtu',1],['unit-other',2]]);
  assert.deepEqual(remote.form.metadata.author.map(a=>[a.id,a.institutionOrderNums]),[['native-author-1',['2']],['native-author-2',['2','1']]]);
  const unitRemote=structuredClone(remote);
  reset('role');const roleId=randomUUID();await run(roleId,'metadata-role-start',true);const roleFolder=await run(roleId,'metadata-role-resume');good(roleFolder);
  assert.equal(remote.form.metadata.author[1].correspondent,true);assert.deepEqual(remote.form.metadata.author.map(a=>a.order),[1,2]);
  assert.deepEqual(remote.form.metadata.authorInstitution,initial().metadata.authorInstitution);const roleRemote=structuredClone(remote);
  reset('absent');const absentId=randomUUID();await run(absentId,'metadata-absent-start',true);const absentFolder=await run(absentId,'metadata-absent-resume');
  const absent=snapshot(absentFolder);assert.equal(absent.tasks[0].stage,'unknown');assert.equal(absent.attempts[0].state,'unknown');assert.equal(absent.tasks[0].record.done,false);
  assert.equal(remote.saves.length,1);assert.equal(remote.completed.length,0);assert.deepEqual(remote.form,initial());
  assert.equal(new Set(runs.map(r=>r.process_id)).size,10,'Every boundary needs a different real application process');
  const evidence={passed:true,synthetic:true,full_process_and_webview_restart:true,runs,author_workspace:authorFolder,unit_workspace:unitFolder,role_workspace:roleFolder,absent_workspace:absentFolder,
   author_remote:authorRemote,unit_remote:unitRemote,role_remote:roleRemote,absent_remote:remote};
  fs.writeFileSync(path.join(authorFolder,'metadata-restart-acceptance.json'),JSON.stringify(evidence,null,2));
  console.log('Native metadata restart PASS: independent server accepted full form -> owned app/WebView2 killed before response -> new process retains exact intent/identity/evidence -> resend/new writes refused before page access -> original role or complete author/institution order read back -> original stage and source restored atomically -> independent SA completed; one edit per scenario.');
  console.log('Unknown safeguards PASS: unrelated remote field change, changed SA value or request received without actual save remains Unknown, keeps original intent, creates no verified proof and performs no further save/SA completion.');
  console.log('Evidence:',path.join(authorFolder,'metadata-restart-acceptance.json'));
 }finally{for(const response of replies)response.destroy();server.close();server.closeAllConnections();}
})().catch(error=>{console.error(error);process.exitCode=1;});
