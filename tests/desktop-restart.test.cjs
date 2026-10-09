// Actual app/WebView2 termination and relaunch. The loopback server owns remote
// state independently; adapters use page components and their ordinary methods.
const http=require('node:http'),fs=require('node:fs'),path=require('node:path');
const {spawn}=require('node:child_process'),{randomUUID,createHash}=require('node:crypto');
const {DatabaseSync}=require('node:sqlite');
const assert=require('node:assert/strict');
const vmScript=require('node:vm').Script;
const fixture=name=>fs.readFileSync(path.join(__dirname,'fixtures',name),'utf8');
const hash=text=>createHash('sha256').update(text).digest('hex');
const root=path.resolve(__dirname,'../runtime/tauri-smoke');
const binary=process.env.DESKTOP_TEST_BINARY||path.resolve(__dirname,'../desktop/src-tauri/target/debug/library-workspace.exe');
function database(folder,read){const db=new DatabaseSync(path.join(folder,'workspace.sqlite3'),{readOnly:true});try{return read(db);}finally{db.close();}}
function snapshot(folder){return database(folder,db=>({tasks:db.prepare('SELECT data FROM tasks').all().map(r=>JSON.parse(r.data)),attempts:db.prepare('SELECT * FROM attempts ORDER BY created,rowid').all().map(r=>({...r,data:JSON.parse(r.data)}))}));}
(async()=>{
 let phase='',remote={batch:null,upload:[],submit:[],push:[]},boundary;
 const replies=new Set();
 const importPage=fixture('import.html').replace('</script>',`
 vm.getData=async function(page,size){this.loading=true;const rows=await(await fetch('/restart/batches')).json();this.tableData=rows.slice((page-1)*size,page*size);this.page.total=rows.length;this.loading=false;};
 const oldFile=drawer.$el.querySelector('input');oldFile.replaceWith(oldFile.cloneNode());
 drawer.$el.querySelector('input').addEventListener('change',async e=>{
   const file=e.target.files[0];const response=await fetch('/restart/upload',{method:'POST',body:JSON.stringify({name:file.name,size:file.size,text:await file.text()})});
   drawer.fileList=[{name:file.name,size:file.size,status:'success',response:await response.json()}];
 });
 drawer.onSubmit=async function(){await fetch('/restart/submit',{method:'POST',body:JSON.stringify({form:this.form,files:this.fileList})});this.drawer=false;};
 push.onSubmit=async function(){await fetch('/restart/push',{method:'POST',body:JSON.stringify(this.form)});this.drawer=false;};
 </script>`);
 const sa=fixture('compare.html').replace('</script>',`
 Object.assign(synthetic,{saLzkId:'restart-import',gh:'001',matchCount:0,itemId:'',reason:'测试缺失'});
 Object.assign(testConfig,{saDoi:'10.1234/test',saWos:'WOS:000123456789012',saClaim:'测试员(001)①'});
 </script>`);
 const library=fixture('library.html').replace('window.testConfig={};','window.testConfig={zero:true};');
 const wos=fixture('wos.html');
 for(const html of [importPage,sa,library,wos])for(const match of html.matchAll(/<script>([\s\S]*?)<\/script>/g))new vmScript(match[1]);
 const server=http.createServer((req,res)=>{
   if(req.url==='/restart/batches'){res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify(remote.batch?[remote.batch]:[]));return;}
   const action={'/restart/upload':'upload','/restart/submit':'submit','/restart/push':'push'}[req.url];
   if(action&&req.method==='POST'){
     let body='';req.on('data',b=>body+=b);req.on('end',()=>{
       const payload=JSON.parse(body);remote[action].push(payload);
       if(action==='submit')remote.batch={id:'restart-batch',modelId:'article-model',batchNumber:'RESTART-TEST',source:'WOS',instructions:payload.form.instructions,total:1,actual:0,fail:0,status:1,increase:0,duplicateSkip:0,duplicateIncreaseUpdate:0,duplicateOverallCoverage:0};
       if(action==='push'){remote.batch.status=2;remote.batch.actual=1;}
       if(action===phase){replies.add(res);res.once('close',()=>replies.delete(res));boundary?.resolve(action);return;}
       res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify({code:200,success:true,data:{name:'restart-server-object.txt'}}));
     });return;
   }
   res.writeHead(200,{'Content-Type':'text/html; charset=utf-8'});
   res.end(req.url==='/native/import'?importPage:req.url==='/native/sa'?sa:req.url==='/advancedSearch'?library:wos);
 });
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 const origin=`http://127.0.0.1:${server.address().port}/`;
 const runs=[];
 async function run(id,next,crash){
   phase=next;const folder=path.join(root,id);
   let resolveBoundary;const accepted=new Promise(resolve=>resolveBoundary=resolve);boundary={resolve:resolveBoundary};
   const env={...process.env,DESKTOP_SMOKE_ORIGIN:origin,DESKTOP_SMOKE_RESTART_ID:id,DESKTOP_SMOKE_RESTART_PHASE:next};
   delete env.DESKTOP_SMOKE_LOSE_UPLOAD_RESPONSE;
   const child=spawn(binary,[],{env,windowsHide:true,stdio:'pipe'});let output='';
   const exited=new Promise((resolve,reject)=>{child.once('exit',(code,signal)=>resolve({code,signal}));child.once('error',reject);});
   for(const stream of [child.stdout,child.stderr])stream.on('data',b=>{output+=b;process.stdout.write(b);});
   let timeout;const timed=new Promise((_,reject)=>{timeout=setTimeout(()=>reject(Error(`Restart ${next} timed out: ${output}`)),150000);});
   try{
     if(crash){
       await Promise.race([accepted,exited.then(result=>{throw Error(`Exited before ${next} crash boundary: ${JSON.stringify(result)} ${output}`);}),timed]);
       const before=snapshot(folder),pending=before.attempts.filter(a=>a.state==='intent');
       assert.equal(pending.length,1);assert.equal(pending[0].action,'import_'+next);
       assert.equal(before.tasks[0].running,true);
       fs.writeFileSync(path.join(folder,`restart-boundary-${next}.json`),JSON.stringify({process_id:child.pid,phase:next,local:before,remote},null,2));
       // Terminate only this harness-owned app and its WebView2 child tree.
       const killer=spawn('taskkill',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'pipe'});
       let killOutput='';killer.stderr.on('data',b=>killOutput+=b);
       const killed=await new Promise((resolve,reject)=>{killer.once('exit',resolve);killer.once('error',reject);});
       assert.equal(killed,0,killOutput);await Promise.race([exited,timed]);
       assert.equal(fs.existsSync(path.join(folder,`restart-${next}.json`)),false,'Process must stop before local completion');
       runs.push({phase:next,process_id:child.pid,terminated:true});
     }else{
       const result=await Promise.race([exited,timed]);
       const report=JSON.parse(fs.readFileSync(path.join(folder,`restart-${next}.json`),'utf8'));
       assert.equal(result.code,0,JSON.stringify(report.failure));assert.equal(report.passed,true,JSON.stringify(report.failure));
       assert.equal(report.process_id,child.pid);runs.push({phase:next,process_id:child.pid,passed:true});
     }
   }finally{clearTimeout(timeout);if(child.exitCode===null&&child.signalCode===null)child.kill();boundary=null;for(const res of replies)res.destroy();}
   return folder;
 }
 try{
   const id=randomUUID();await run(id,'submit',true);await run(id,'push',true);const folder=await run(id,'verify',false);
   for(const action of ['upload','submit','push'])assert.equal(remote[action].length,1,action+' repeated after full restart');
   const final=snapshot(folder);assert.equal(final.tasks[0].stage,'pushed');assert.equal(final.tasks[0].record.done,false);
   assert.equal(final.attempts.length,3);assert.ok(final.attempts.every(a=>a.state==='verified'));
   for(const action of ['submit','push']){
     const original=JSON.parse(fs.readFileSync(path.join(folder,`restart-boundary-${action}.json`),'utf8'));
     assert.deepEqual(final.attempts.find(a=>a.action==='import_'+action).data.payload,original.local.attempts.find(a=>a.action==='import_'+action).data.payload);
   }
   const task=final.tasks[0];assert.equal(hash(fs.readFileSync(task.artifact.path)),task.artifact.candidate.sha256);
   assert.equal(hash(remote.upload[0].text),task.artifact.candidate.sha256);
   assert.equal(remote.submit[0].form.instructions,'SA补充-restart-import');assert.equal(remote.submit[0].form.datasetId,'sjtu-1');
   assert.deepEqual(remote.push[0],{batchId:'restart-batch',modelId:'article-model',duplicateChecking:true,duplicateQueryType:'ppt-composite',duplicateItemProcessingType:'4',newItemProcessingType:'1',owner:true,updateFields:[]});
   assert.equal(fs.readdirSync(path.join(folder,'downloads')).length,1);
   const importRemote=structuredClone(remote);
   remote={batch:null,upload:[],submit:[],push:[]};
   const uploadId=randomUUID();await run(uploadId,'upload',true);const uploadFolder=await run(uploadId,'upload-readback',false);
   const unknown=snapshot(uploadFolder);assert.equal(unknown.tasks[0].stage,'unknown');assert.equal(unknown.attempts.length,1);assert.equal(unknown.attempts[0].state,'unknown');
   assert.equal(remote.upload.length,1);assert.equal(remote.submit.length,0);assert.equal(remote.push.length,0);
   assert.equal(hash(fs.readFileSync(unknown.tasks[0].artifact.path)),unknown.attempts[0].data.payload.contentSha);
   assert.equal(new Set(runs.map(r=>r.process_id)).size,5,'Each phase needs a different app process');
   const evidence={passed:true,synthetic:true,full_process_and_webview_restart:true,runs,import_workspace:folder,upload_workspace:uploadFolder,import_remote:importRemote,unknown_upload_remote:remote};
   fs.writeFileSync(path.join(folder,'restart-acceptance.json'),JSON.stringify(evidence,null,2));
   console.log('Native process restart PASS: server accepted import/push -> owned app/WebView2 tree forcibly terminated -> fresh app startup -> original immutable intent and archive retained -> resend refused -> changed archive refused -> restored archive + actual batch readback -> Imported/Pushed -> repeated reads write-free. Upload/import/push each once.');
   console.log('Lost upload window PASS: server accepted TXT -> full app crash -> new process/window has no batch -> remains Unknown -> upload and import repeats refused. Independent recovery without a server receipt is not claimed.');
   console.log('Evidence:',path.join(folder,'restart-acceptance.json'));
 }finally{for(const res of replies)res.destroy();server.close();server.closeAllConnections();}
})().catch(error=>{console.error(error);process.exitCode=1;});
