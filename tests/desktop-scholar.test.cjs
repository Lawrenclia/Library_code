const {chromium}=require('playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const {runScholarCommand}=require('../desktop/src-tauri/browser/scholar-adapter.cjs');
(async()=>{
 const browser=await chromium.launch({executablePath:process.env.TEST_BROWSER||'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
 try{
  const context=await browser.newContext();
  const fixture=fs.readFileSync(path.join(__dirname,'fixtures/scholar.html'),'utf8');
  await context.route('**/*',r=>r.fulfill({status:200,contentType:'text/html',body:fixture}));
  const page=await context.newPage();
  const reset=async()=>{await page.goto('http://admin.ir.lib.sjtu.edu.cn/#/scholar/list');await page.reload();};
  const cmd=(action,extra={})=>({action,sa_id:'demo-001',staff_id:'00001',expires:Date.now()+45000,...extra});
  const run=c=>page.evaluate(runScholarCommand,c);
  const prepare=async()=>{const r=await run(cmd('alias_read'));assert.equal(r.ok,true,JSON.stringify(r));return r.data;};
  const save=p=>run(cmd('alias_add',{confirmed:true,expected_scholar:p.scholar,expected_aliases:p.aliases,alias:'Demo, X'}));
  const cases=[];const test=(name,fn)=>cases.push([name,fn]);
  test('exact staff lookup and alias read perform no writes',async()=>{const p=await prepare();assert.equal(p.scholar.wno,'00001');assert.equal(await page.evaluate(()=>aliasWrites),0);});
  test('source name saves once, reads persisted ID, retains default names',async()=>{const p=await prepare();const r=await save(p);assert.equal(r.ok,true,JSON.stringify(r));assert.equal(r.data.verified,true);assert.equal(r.data.alias.id,'alias-001');assert.equal(r.data.alias.defaultNameCn,0);assert.equal(await page.evaluate(()=>aliasWrites),1);});
  test('existing alias is verified without adding again',async()=>{const p=await prepare();await save(p);const fresh=await prepare();const r=await save(fresh);assert.equal(r.data.already_present,true);assert.equal(await page.evaluate(()=>aliasWrites),1);});
  test('key reordering across IPC does not make identity falsely stale',async()=>{const p=await prepare();p.scholar={nameEn:p.scholar.nameEn,nameCn:p.scholar.nameCn,wno:p.scholar.wno,id:p.scholar.id};assert.equal((await save(p)).ok,true);});
  test('different staff record and ambiguous people never write',async()=>{await page.evaluate(()=>testConfig.people=[{id:'x',wno:'00002'}]);assert.equal((await run(cmd('alias_read'))).code,'IDENTITY_CONFLICT');assert.equal(await page.evaluate(()=>aliasWrites),0);});
  test('numeric scholar IDs are refused before alias editing',async()=>{await page.evaluate(()=>testConfig.people=[{id:123,wno:'00001'}]);assert.equal((await run(cmd('alias_read'))).ok,false);assert.equal(await page.evaluate(()=>aliasWrites),0);});
  test('changed aliases and missing confirmation cannot save',async()=>{const p=await prepare();await page.evaluate(()=>persistedAliases.push({id:'other',scholarId:'scholar-001',nameAlias:'Other Name',defaultNameCn:1,defaultNameEn:0,isSet:false}));assert.equal((await save(p)).code,'TASK_CHANGED');assert.equal(await page.evaluate(()=>aliasWrites),0);});
  test('foreign modal prevents query and save',async()=>{await page.evaluate(()=>document.body.insertAdjacentHTML('beforeend','<div class="el-dialog">Foreign edit</div>'));assert.equal((await run(cmd('alias_read'))).code,'REVIEW_REQUIRED');assert.equal(await page.evaluate(()=>aliasWrites),0);});
  test('unknown save is marked submitted; only read back can recover',async()=>{const p=await prepare();await page.evaluate(()=>testConfig.hangSave=true);const r=await run(cmd('alias_add',{confirmed:true,expected_scholar:p.scholar,expected_aliases:p.aliases,alias:'Demo, X',expires:Date.now()+4200}));assert.equal(r.ok,false);assert.equal(r.submitted,true);assert.equal(r.code,'REMOTE_RESULT_UNKNOWN');assert.equal(await page.evaluate(()=>aliasWrites),1);});
  test('read-only recovery checks exact saved alias on same scholar',async()=>{await page.evaluate(()=>persistedAliases=[{id:'saved',scholarId:'scholar-001',nameAlias:'Demo, X',defaultNameCn:0,defaultNameEn:0,isSet:false}]);const r=await run(cmd('alias_check',{alias:'Demo, X',expected_scholar_id:'scholar-001'}));assert.equal(r.data.verified,true);assert.equal(await page.evaluate(()=>aliasWrites),0);});
  test('unverified absence cannot authorize a repeated save',async()=>{const r=await run(cmd('alias_check',{alias:'Demo, X',expected_scholar_id:'scholar-001'}));assert.equal(r.code,'REMOTE_RESULT_UNKNOWN');assert.equal(await page.evaluate(()=>aliasWrites),0);});
  test('foreign host and expired commands are refused',async()=>{await page.goto('http://example.test/#/scholar/list');assert.equal((await run(cmd('alias_read'))).code,'AUTH_REQUIRED');await reset();assert.equal((await run(cmd('alias_read',{expires:0}))).ok,false);});
  test('metadata identity read closes its own clean window and next SA can query without reusing identity',async()=>{
    const first=await run(cmd('alias_read',{close_after_read:true}));assert.equal(first.ok,true,JSON.stringify(first));
    assert.equal(await page.evaluate(()=>aliasModal.dialogVisible),false);
    const second=await run(cmd('alias_read',{sa_id:'next-sa',close_after_read:true}));assert.equal(second.ok,true,JSON.stringify(second));
    assert.equal(second.data.scholar.wno,'00001');assert.equal(await page.evaluate(()=>aliasWrites),0);assert.equal(await page.evaluate(()=>aliasModal.dialogVisible),false);
  });
  test('read-and-close never discards a manual draft, another task window or in-flight save',async()=>{
    await prepare();await page.evaluate(()=>aliasModal.data.unshift({isSet:true,id:'',scholarId:'scholar-001',nameAlias:'manual draft'}));
    assert.equal((await run(cmd('alias_read',{close_after_read:true}))).code,'REVIEW_REQUIRED');assert.equal(await page.evaluate(()=>aliasModal.data[0].nameAlias),'manual draft');
    await reset();await prepare();assert.equal((await run(cmd('alias_read',{sa_id:'different-sa',close_after_read:true}))).code,'REVIEW_REQUIRED');
    assert.equal(await page.evaluate(()=>aliasModal.dialogVisible),true);
    await reset();await prepare();await page.evaluate(()=>aliasModal.loading=true);
    assert.equal((await run(cmd('alias_read',{close_after_read:true}))).code,'PAGE_TIMEOUT');assert.equal(await page.evaluate(()=>aliasModal.dialogVisible),true);
    assert.equal(await page.evaluate(()=>aliasWrites),0);
  });
  for(const [name,fn] of cases){await reset();await fn();console.log('PASS '+name);}
  console.log(`Desktop scholar: ${cases.length} isolated UI contract checks passed; no live writes.`);
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
