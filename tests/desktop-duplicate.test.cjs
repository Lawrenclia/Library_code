const {chromium}=require('playwright'),fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const {runDuplicateCommand}=require('../desktop/src-tauri/browser/duplicate-adapter.cjs');
(async()=>{
 const browser=await chromium.launch({executablePath:process.env.TEST_BROWSER||'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
 try{
  const context=await browser.newContext(),fixture=fs.readFileSync(path.join(__dirname,'fixtures/duplicate.html'),'utf8');
  await context.route('**/*',r=>r.fulfill({status:200,contentType:'text/html',body:fixture}));
  const page=await context.newPage();
  const reset=async()=>{await page.goto('http://admin.ir.lib.sjtu.edu.cn/#/collectItem/duplicateData');await page.reload();};
  const cmd=(action,extra={})=>({action,sa_id:'sa-001',title:'A paper',sa_ids:['item-primary'],expected_sa_ids:['item-primary','item-source'],expires:Date.now()+45000,...extra});
  const run=c=>page.evaluate(runDuplicateCommand,c);
  const scan=async()=>{const r=await run(cmd('duplicate_scan'));assert.equal(r.ok,true,JSON.stringify(r));return r.data;};
  const prepare=async()=>{const s=await scan();const r=await run(cmd('duplicate_read',{group_id:s.groups[0].id,expected_group:s.groups[0],expected_threshold:s.title_similarity}));assert.equal(r.ok,true,JSON.stringify(r));return r.data;};
  const save=(p,extra={})=>run(cmd('duplicate_merge',{group_id:p.group.id,expected_group:p.group,expected_threshold:p.title_similarity,source_id:'item-source',target_id:'item-primary',confirmed:true,...extra}));
  const cases=[],test=(name,fn)=>cases.push([name,fn]);
  test('candidate read retains actual threshold and never merges automatically',async()=>{const p=await prepare();assert.equal(p.title_similarity,93.5);assert.equal(p.group.items.length,2);assert.equal(await page.evaluate(()=>mergeWrites.length),0);});
  test('explicit cross-model source merges once and reads actual master fields',async()=>{const p=await prepare();const r=await save(p);assert.equal(r.ok,true,JSON.stringify(r));assert.equal(r.data.server_success,true);assert.equal(r.data.master_after.id,'item-primary');assert.equal(r.data.master_after.fields.doi[0],'10.1234/test');assert.deepEqual(await page.evaluate(()=>mergeWrites),[{itemId:'item-source',targetItemId:'item-primary'}]);});
  test('master may be the other candidate, not always the first record',async()=>{const p=await prepare();const r=await save(p,{source_id:'item-primary',target_id:'item-source'});assert.equal(r.ok,true,JSON.stringify(r));assert.equal(r.data.master_after.id,'item-source');});
  test('IPC key order does not change candidate identity',async()=>{const p=await prepare();p.group={items:p.group.items.map(i=>({metadata:i.metadata,model_name:i.model_name,id:i.id})),primary_id:p.group.primary_id,id:p.group.id};assert.equal((await save(p)).ok,true);});
  test('same source and target or missing confirmation never writes',async()=>{const p=await prepare();assert.equal((await save(p,{source_id:'item-primary'})).ok,false);assert.equal((await save(p,{confirmed:false})).ok,false);assert.equal(await page.evaluate(()=>mergeWrites.length),0);});
  test('changed metadata and changed query settings stop before sending',async()=>{const p=await prepare();await page.evaluate(()=>testGroups[0].items[0].item.metadata.doi=['10.9999/changed']);assert.equal((await save(p)).code,'TASK_CHANGED');assert.equal(await page.evaluate(()=>mergeWrites.length),0);});
  test('numeric identifiers and stale query results are refused',async()=>{await page.evaluate(()=>testGroups[0].item.id=123);assert.equal((await run(cmd('duplicate_scan'))).code,'IDENTITY_CONFLICT');await reset();await page.evaluate(()=>testConfig.staleQuery=true);assert.equal((await run(cmd('duplicate_scan'))).ok,false);});
  test('foreign modal and wrong confirmation cannot merge',async()=>{await page.evaluate(()=>document.body.insertAdjacentHTML('beforeend','<div class="el-dialog">Manual edit</div>'));assert.equal((await run(cmd('duplicate_scan'))).code,'REVIEW_REQUIRED');await reset();const p=await prepare();await page.evaluate(()=>testConfig.wrongConfirm=true);const r=await save(p);assert.equal(r.submitted,false);assert.equal(await page.evaluate(()=>mergeWrites.length),0);});
  test('timeout after confirmation is unknown, not a safe failure',async()=>{const p=await prepare();await page.evaluate(()=>testConfig.hangMerge=true);const r=await save(p,{expires:Date.now()+4200});assert.equal(r.code,'REMOTE_RESULT_UNKNOWN');assert.equal(r.submitted,true);assert.equal(await page.evaluate(()=>mergeWrites.length),1);});
  test('success receipt without source removal remains unknown',async()=>{const p=await prepare();await page.evaluate(()=>testConfig.retainSource=true);const r=await save(p);assert.equal(r.code,'REMOTE_RESULT_UNKNOWN');assert.equal(r.submitted,true);});
  test('wrong or inaccessible master never counts as verified merge',async()=>{const p=await prepare();await page.evaluate(()=>testConfig.wrongMaster=true);const r=await save(p);assert.equal(r.ok,false);assert.equal(r.submitted,true);});
  test('read-only recovery checks exact SA changes, pool and master',async()=>{const p=await prepare();await save(p);const c=cmd('duplicate_check',{source_id:'item-source',target_id:'item-primary',expected_master:p.group.items[0],sa_verified:true});const r=await run(c);assert.equal(r.ok,true,JSON.stringify(r));assert.equal(r.data.verified,true);assert.equal(await page.evaluate(()=>mergeWrites.length),1);});
  test('partial merge among three SA records can recover without merging the remaining record',async()=>{await page.evaluate(()=>{const third={...clone(primary),id:'third'};testGroups[0].items.push({item:third});testMasters.third=third;});const p=await prepare();await save(p);const r=await run(cmd('duplicate_check',{source_id:'item-source',target_id:'item-primary',expected_master:p.group.items[0],sa_verified:true,expected_sa_ids:['item-primary','item-source','third'],sa_ids:['item-primary','third']}));assert.equal(r.ok,true,JSON.stringify(r));assert.equal(await page.evaluate(()=>mergeWrites.length),1);});
  test('an unrelated SA edit cannot verify the previous merge',async()=>{const p=await prepare();await save(p);const r=await run(cmd('duplicate_check',{source_id:'item-source',target_id:'item-primary',expected_master:p.group.items[0],sa_verified:true,sa_ids:['item-primary','unexpected']}));assert.equal(r.code,'REMOTE_RESULT_UNKNOWN');});
  test('unverified SA and a still-present source cannot authorize recovery',async()=>{const p=await prepare();assert.equal((await run(cmd('duplicate_check',{source_id:'item-source',target_id:'item-primary',expected_master:p.group.items[0],sa_verified:false}))).ok,false);assert.equal((await run(cmd('duplicate_check',{source_id:'item-source',target_id:'item-primary',expected_master:p.group.items[0],sa_verified:true}))).code,'REMOTE_RESULT_UNKNOWN');assert.equal(await page.evaluate(()=>mergeWrites.length),0);});
  test('matching title never hides lost author or publication fields during recovery',async()=>{
    const p=await prepare();await save(p);await page.evaluate(()=>delete testMasters['item-primary'].metadata.author);
    const r=await run(cmd('duplicate_check',{source_id:'item-source',target_id:'item-primary',expected_master:p.group.items[0],expected_source:p.group.items[1],sa_verified:true}));
    assert.equal(r.ok,false);assert.equal(r.code,'REMOTE_RESULT_UNKNOWN');assert.match(r.error,/author/);assert.equal(await page.evaluate(()=>mergeWrites.length),1);
  });
  test('model changes and unbacked extra fields cannot verify a merged master',async()=>{
    for(const model of [true,false]){await reset();const p=await prepare();await save(p);await page.evaluate(flag=>{if(flag)testMasters['item-primary'].modelName='Wrong model';else testMasters['item-primary'].metadata.abstract=['Unbacked new content'];},model);
      assert.equal((await run(cmd('duplicate_check',{source_id:'item-source',target_id:'item-primary',expected_master:p.group.items[0],expected_source:p.group.items[1],sa_verified:true}))).code,'REMOTE_RESULT_UNKNOWN');assert.equal(await page.evaluate(()=>mergeWrites.length),1);
    }
  });
  test('retained original fields accept known alternate title and source-only new data',async()=>{
    const p=await prepare();await save(p);p.group.items[1].metadata.pages=['10-20'];await page.evaluate(()=>{testMasters['item-primary'].metadata.title.push('A paper corrected');testMasters['item-primary'].metadata.pages=['10-20'];});
    const r=await run(cmd('duplicate_check',{source_id:'item-source',target_id:'item-primary',expected_master:p.group.items[0],expected_source:p.group.items[1],sa_verified:true}));assert.equal(r.ok,true,JSON.stringify(r));assert.equal(await page.evaluate(()=>mergeWrites.length),1);
  });
  test('all pages are read and an earlier-page group can be selected',async()=>{await page.evaluate(()=>{const g=testGroups[0];for(let i=2;i<=51;i++)testGroups.push({id:'group-'+i,item:{...clone(g.item),id:'primary-'+i},items:[{item:{...clone(g.items[0].item),id:'source-'+i}}]});});const p=await prepare();assert.equal(await page.evaluate(()=>queryCalls.some(q=>q.page===2)),true);assert.equal(p.group.id,'group-1');});
  test('large and incomplete candidate sets are not silently truncated',async()=>{await page.evaluate(()=>{const g=testGroups[0];for(let i=2;i<=201;i++)testGroups.push({id:'group-'+i,item:{...clone(g.item),id:'primary-'+i},items:[]});});assert.equal((await run(cmd('duplicate_scan'))).code,'AMBIGUOUS_RESULT');await reset();await page.evaluate(()=>testConfig.dropPage=true);assert.equal((await run(cmd('duplicate_scan'))).ok,false);});
  for(const [name,fn]of cases){await reset();await fn();console.log('PASS '+name);}
  console.log(`Desktop duplicate: ${cases.length} isolated UI contract checks passed; no live writes.`);
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
