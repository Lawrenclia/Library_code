// Real Tauri/WebView2, SQLite, page IPC and native download events. Loopback only.
const http=require('node:http'),fs=require('node:fs'),path=require('node:path');
const {spawn}=require('node:child_process');
const assert=require('node:assert/strict');
(async()=>{
 const fixture=fs.readFileSync(path.join(__dirname,'fixtures/wos.html'),'utf8').replace('searches++;history.pushState',`if(main.querySelector('input').value.startsWith('Missing')){main.insertAdjacentHTML('afterbegin','<p>Your search found no results</p>');return;}searches++;history.pushState`).replace('<script>',`<script>window.addEventListener('unhandledrejection',e=>fetch('/diagnostic',{method:'POST',body:String(e.reason?.message||e.reason)}));window.addEventListener('error',e=>fetch('/diagnostic',{method:'POST',body:String(e.message)}));fetch('/diagnostic',{method:'POST',body:'loaded: '+location.pathname+'; ipc='+typeof window.__TAURI_INTERNALS__});`);
 let mergeCount=0,metadataCount=0,zeroLinkCount=0,zeroClaimCount=0,zeroCompleteCount=0;
 const importReceipts={upload:[],submit:[],push:[]},metadataReceipts=[];let fullImportComplete=false;
 const sa=fs.readFileSync(path.join(__dirname,'fixtures/compare.html'),'utf8').replace('姓名：测试员<br/>是否通讯作者：是','姓名：测试员<br/>工号：001<br/>是否第一作者：是<br/>是否通讯作者：是').replace('</script>',`
 Object.assign(synthetic,{saLzkId:'smoke-duplicate',gh:'001',matchCount:2,itemId:',item-primary,item-source',reason:'重复数据'});
 const originalQuery=vm.getData.bind(vm);
 vm.getData=async function(){
  if(vm.searchForm.saLzkId==='smoke-metadata'){
   Object.assign(synthetic,{saLzkId:'smoke-metadata',itemId:'item-metadata',gh:'001',matchCount:1,reason:'通讯作者标记不一致；第一作者标记不一致'});
   if(!window.startedMetadata){synthetic.markStatus='待处理';synthetic.remark='';window.startedMetadata=true;}
   testConfig.claim='已认领';
  }else if(vm.searchForm.saLzkId==='smoke-order'){
   Object.assign(synthetic,{saLzkId:'smoke-order',itemId:'item-order',gh:'001',matchCount:1,reason:'第一作者标记不一致；交大是否第一单位不一致'});
   if(!window.startedOrder){synthetic.markStatus='待处理';synthetic.remark='';window.startedOrder=true;}
   testConfig.claim='已认领';
  }else if(vm.searchForm.saLzkId==='smoke-existing'){
   Object.assign(synthetic,{saLzkId:'smoke-existing',itemId:'item-existing',matchCount:1,reason:'通讯作者标记不一致；第一作者标记不一致'});
   testConfig.claim='已认领';testConfig.authorInfo='署名：Tester<br/>工号：001<br/>是否第一作者：否<br/>是否通讯作者：否';
  }else{Object.assign(synthetic,{saLzkId:'smoke-duplicate',gh:'001',markStatus:'待处理',remark:'',reason:'重复数据'});const state=await (await fetch('/native/state')).json();synthetic.itemId=state.merged?'item-primary':',item-primary,item-source';synthetic.matchCount=state.merged?1:2;}
  return originalQuery();
 };
 ${fs.readFileSync(path.join(__dirname,'fixtures/metadata-native.js'),'utf8')}
 ${fs.readFileSync(path.join(__dirname,'fixtures/zero-native.js'),'utf8')}
 ${fs.readFileSync(path.join(__dirname,'fixtures/version-native.js'),'utf8')}
 </script>`);
 const scholar=fs.readFileSync(path.join(__dirname,'fixtures/scholar.html'),'utf8').replaceAll("'00001'","'001'");
 const duplicate=fs.readFileSync(path.join(__dirname,'fixtures/duplicate.html'),'utf8').replaceAll('A paper','Synthetic paper').replace('</script>',`
 const originalEmit=modal.$emit.bind(modal);
 modal.$emit=function(event){if(event==='upData'){fetch('/native/merged',{method:'POST'}).then(response=>{if(response.ok)originalEmit(event);});}else originalEmit(event);};
 </script>`);
 const library=fs.readFileSync(path.join(__dirname,'fixtures/library.html'),'utf8').replace('window.testConfig={};','window.testConfig={zero:true};').replace('</script>',`
 const nativeFrontQuery=vm.advanceSubmit.bind(vm);
 vm.advanceSubmit=async function(params){
  const state=await (await fetch('/native/import-state')).json();
  if(state.active){testConfig.zero=false;testConfig.rows=[{id:'1244586319225556123',modelName:'期刊论文',metadata:{title:['Synthetic paper'],doi:['10.1234/test'],wosId:['WOS:000123456789012'],abstract:['完整合成摘要'],author:[{fullname:'Tester',order:1}]}}];}
  else if(window.fullPipelineFront){testConfig.zero=true;delete testConfig.rows;}
  window.fullPipelineFront=state.active;return nativeFrontQuery(params);
 };
 </script>`);
 const importPage=fs.readFileSync(path.join(__dirname,'fixtures/import.html'),'utf8').replace('</script>',fs.readFileSync(path.join(__dirname,'fixtures/import-native.js'),'utf8')+'</script>');
 for(const html of [fixture,sa,duplicate,scholar,library,importPage])for(const match of html.matchAll(/<script>([\s\S]*?)<\/script>/g))new(require('node:vm').Script)(match[1]);
 const server=http.createServer((req,res)=>{
  if(req.url==='/diagnostic'){let body='';req.on('data',b=>body+=b);req.on('end',()=>console.log('Fixture diagnostic:',body));res.writeHead(200);res.end();return;}
  if(req.url==='/native/state'){res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify({merged:mergeCount>0}));return;}
  if(req.url==='/native/merged'&&req.method==='POST'){mergeCount++;res.writeHead(200);res.end();return;}
  if(req.url==='/native/metadata-save'&&req.method==='POST'){let body='';req.on('data',b=>body+=b);req.on('end',()=>{metadataCount++;metadataReceipts.push(JSON.parse(body));res.writeHead(200);res.end();});return;}
  if(req.url==='/native/zero-link'&&req.method==='POST'){zeroLinkCount++;res.writeHead(200);res.end();return;}
  if(req.url==='/native/zero-claim'&&req.method==='POST'){zeroClaimCount++;res.writeHead(200);res.end();return;}
  if(req.url==='/native/zero-complete'&&req.method==='POST'){zeroCompleteCount++;if(importReceipts.push.length&&!fullImportComplete)fullImportComplete=true;res.writeHead(200);res.end();return;}
  if(req.url==='/native/import-state'){res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify({active:importReceipts.push.length>0&&!fullImportComplete}));return;}
  const importAction={'/native/import-upload':'upload','/native/import-submit':'submit','/native/import-push':'push'}[req.url];
  if(importAction&&req.method==='POST'){let body='';req.on('data',b=>body+=b);req.on('end',()=>{importReceipts[importAction].push(JSON.parse(body));res.writeHead(200);res.end();});return;}
  res.writeHead(200,{'Content-Type':'text/html; charset=utf-8'});res.end(req.url==='/native/sa'?sa:req.url==='/native/import'?importPage:req.url==='/native/duplicate'?duplicate:req.url==='/native/scholar'?scholar:req.url==='/advancedSearch'?library:fixture);
 });
 await new Promise(r=>server.listen(0,'127.0.0.1',r));
 const root=path.resolve(__dirname,'../runtime/tauri-smoke');fs.mkdirSync(root,{recursive:true});const before=new Set(fs.readdirSync(root));
 const binary=process.env.DESKTOP_TEST_BINARY||path.resolve(__dirname,'../desktop/src-tauri/target/debug/library-workspace.exe');
 const child=spawn(binary,[],{env:{...process.env,DESKTOP_SMOKE_ORIGIN:`http://127.0.0.1:${server.address().port}/`,DESKTOP_SMOKE_LOSE_UPLOAD_RESPONSE:'1'},windowsHide:true,stdio:'pipe'});
 let output='';child.stdout.on('data',b=>{output+=b;process.stdout.write(b);});child.stderr.on('data',b=>{output+=b;process.stderr.write(b);});
 const timer=setTimeout(()=>child.kill(),240000);
 try{
  const code=await new Promise((resolve,reject)=>{child.once('exit',resolve);child.once('error',reject);});
  const created=fs.readdirSync(root).filter(n=>!before.has(n)&&fs.existsSync(path.join(root,n,'native-smoke.json'))&&JSON.parse(fs.readFileSync(path.join(root,n,'native-smoke.json'),'utf8')).process_id===child.pid);assert.equal(created.length,1,output);
  const report=JSON.parse(fs.readFileSync(path.join(root,created[0],'native-smoke.json'),'utf8'));
  const summary=JSON.stringify({failure:report.failure,tasks:report.workspace?.tasks?.map(t=>({id:t.id,stage:t.stage,error:t.last_error}))});
  assert.equal(code,0,summary);assert.equal(report.passed,true,summary);
  assert.equal(mergeCount,1,'The native browser must merge once, then reject stale preparation.');
  assert.equal(metadataCount,4,'Two role controls and two complete order operations must each save once; stale repeats must not write.');
  const orderWrites=metadataReceipts.filter(r=>r.form.id==='item-order');assert.equal(orderWrites.length,2);
  assert.deepEqual(orderWrites[0].form.metadata.author.map(a=>[a.id,a.order,a.commonFirst]),[['native-author-2',1,false],['native-author-1',2,false]]);
  assert.deepEqual(orderWrites[1].form.metadata.authorInstitution.map(u=>[u.id,u.order]),[['unit-sjtu',1],['unit-other',2]]);
  assert.deepEqual(orderWrites[1].form.metadata.author.map(a=>[a.id,a.institutionOrderNums]),[['native-author-2',['2','1']],['native-author-1',['2']]]);
  assert.equal(orderWrites[1].form.metadata.abstract[0],'完整原文摘要仍保留');
  assert.equal(zeroLinkCount,3,'Full import, corrected and seeded pushed tasks each link once; repeat links only read back.');
  assert.equal(zeroClaimCount,1,'The shared scholar relationship is claimed once, not once per SA.');
  assert.equal(zeroCompleteCount,3,'Each independent SA task completes once only after live issue reviews.');
  for(const key of ['upload','submit','push'])assert.equal(importReceipts[key].length,1,'Import '+key+' must be submitted exactly once across recovery.');
  assert.equal(importReceipts.upload[0].name,'SA-WOS-smoke-import.txt');
  assert.match(importReceipts.upload[0].text,/WOS:000123456789012/);
  assert.equal(importReceipts.submit[0].form.instructions,'SA补充-smoke-import');
  assert.equal(importReceipts.submit[0].form.datasetId,'sjtu-1');
  assert.deepEqual(importReceipts.push[0],{batchId:'batch-001',modelId:'article-model',duplicateChecking:true,duplicateQueryType:'ppt-composite',duplicateItemProcessingType:'4',newItemProcessingType:'1',owner:true,updateFields:[]});
  fs.writeFileSync(path.join(root,created[0],'native-import-receipts.json'),JSON.stringify(importReceipts,null,2));
  fs.writeFileSync(path.join(root,created[0],'native-metadata-receipts.json'),JSON.stringify(metadataReceipts,null,2));
  console.log('Native Tauri: three zero results -> fourth search -> Full Record -> native TXT -> Rust identity -> complete source evidence -> cross-SA reuse without browser -> SQLite -> source Excel passed.');
  console.log('Native duplicate: SA -> full candidate group -> explicit main/source -> platform merge receipt -> fresh master/pool -> atomic SQLite proof -> fresh SA -> stale repeat rejected.');
  console.log('Native existing: fresh multi-reason plan -> independent source conclusions -> completion blocked with one unresolved -> final fresh read -> SA processed with both notes.');
  console.log('Native metadata: exact staff -> full editor snapshot -> explicit author -> corresponding/common-first saves -> full metadata and SA readback -> stale repeat blocked -> SA completed.');
  console.log('Native complete order: original author IDs and flags -> author reorder -> institution reorder and all author references -> four full before/after receipts including role edits -> SA values verified -> original intent resolved -> independent SA completion; loopback fixtures only.');
  console.log('Native library: absence checkbox refused -> all three front queries -> complete evidence -> real candidate ID selection -> existing branch blocks new import.');
  console.log('Native zero-to-unique: corrected-existing and a seeded verified-push checkpoint -> actual link -> full live issue plan -> source conclusions -> exact-staff claim/readback -> independent SA completions; roster match counts remain zero. Upload/push is not exercised in this checkpoint test.');
  console.log('Native link recovery: fixture write -> deliberately omitted local finish -> SQLite reopen/startup recovery -> unknown blocks resend -> read-only SA check -> original pushed stage and immutable payload restored. WebView2/process restart is not exercised.');
  console.log('Native input versions: changed roster retains old task -> SQLite reopen keeps proposal -> old review blocked -> fresh SA preparation -> superseded proposal rejected -> second fresh SA -> atomic acceptance, history retained and business conclusions reset. No platform write is performed for version acceptance.');
  console.log('Native full import: verified TXT -> fresh front absence and SA -> upload -> one import -> SQLite reopen/unknown blocks resend -> batch content readback -> PPT push -> SQLite reopen/unknown blocks resend -> pushed content readback -> actual front item -> SA link -> exact-staff claim -> independent reasons -> processed SA. All platform pages are local synthetic fixtures; process restart and live login are not exercised.');
  console.log('Native unknown upload: fixture server receives the file once -> confirmation deliberately replaced with an unknown result -> SQLite reopen/startup recovery -> repeat upload refused -> original file bytes, form, institution and server response read back -> atomic Uploaded checkpoint -> same drawer imports once. The upload window survives; full process restart/lost-window recovery is not claimed.');
  console.log('Evidence:',path.join(root,created[0],'native-smoke.json'));
 }finally{clearTimeout(timer);server.close();}
})().catch(e=>{console.error(e);process.exit(1);});
