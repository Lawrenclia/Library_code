const {chromium}=require('playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const {runLibraryCommand}=require('../desktop/src-tauri/browser/library-adapter.cjs');
(async()=>{
 const browser=await chromium.launch({executablePath:process.env.TEST_BROWSER||'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
 try{
  const context=await browser.newContext();const fixture=fs.readFileSync(path.join(__dirname,'fixtures/library.html'),'utf8');
  await context.route('**/*',r=>r.fulfill({status:200,contentType:'text/html',body:fixture}));const page=await context.newPage();
  const reset=async()=>{await page.goto('http://www.ir.lib.sjtu.edu.cn/advancedSearch');await page.reload();};
  const command=(extra={})=>({action:'library_search',sa_id:'sa-library',title:'Correct paper title',doi:'',wos:'',expires:Date.now()+10000,...extra});
  const run=c=>page.evaluate(runLibraryCommand,c);
  const cases=[];const test=(name,fn)=>cases.push([name,fn]);
  test('all pages, exact text IDs and full metadata retained, no candidate selected',async()=>{const r=await run(command());assert.equal(r.ok,true,JSON.stringify(r));assert.equal(r.data.items.length,12);assert.equal(r.data.queries[0].pages.length,2);assert.equal(r.data.items[0].id,'1244586319225556000');assert.equal(r.data.items[0].metadata.abstract[0],'完整摘要必须保留');assert.equal(r.data.platform_id,undefined);assert.equal(await page.evaluate(()=>searches[0].queryFields[0].precise),false);});
  test('title DOI WOS each explicitly return zero, unrelated filters reset',async()=>{await page.evaluate(()=>{testConfig.zero=true;vm.ruleForm.containFullText='YES';vm.ruleForm.fieldFilters=[['modelId','1']];});const r=await run(command({doi:'10.1234/test',wos:'WOS:001234567890123'}));assert.equal(r.ok,true,JSON.stringify(r));assert.equal(r.data.items.length,0);assert.deepEqual(r.data.queries.map(q=>q.kind),['title','doi','wos']);assert.equal(await page.evaluate(()=>searches.every(s=>s.containFullText===''&&s.fieldFilters.length===0)),true);});
  test('any missing identifier field blocks before sending partial searches',async()=>{await page.evaluate(()=>modal.retrievalFieldList.pop());const r=await run(command({wos:'WOS:001234567890123'}));assert.equal(r.code,'PAGE_UNSUPPORTED');assert.equal(await page.evaluate(()=>searches.length),0);});
  test('swallowed request failure cannot become zero results',async()=>{await page.evaluate(()=>testConfig.failQuery=true);const r=await run(command({expires:Date.now()+2400}));assert.equal(r.code,'PAGE_TIMEOUT');assert.equal(r.submitted,false);});
  test('unrelated query event cannot become evidence',async()=>{await page.evaluate(()=>testConfig.foreignEvent=true);const r=await run(command({expires:Date.now()+2400}));assert.equal(r.code,'PAGE_TIMEOUT');});
  test('changed query conditions cannot become evidence',async()=>{await page.evaluate(()=>testConfig.changeForm=true);assert.equal((await run(command())).code,'TASK_CHANGED');});
  test('repeated records on different pages block incomplete pagination',async()=>{await page.evaluate(()=>testConfig.repeatPage=true);assert.equal((await run(command())).code,'TASK_CHANGED');});
  test('changing total during paging requires fresh query',async()=>{await page.evaluate(()=>testConfig.changeTotal=true);assert.equal((await run(command())).code,'TASK_CHANGED');});
  test('truncated result pages are not accepted',async()=>{await page.evaluate(()=>testConfig.truncate=true);assert.equal((await run(command())).code,'PAGE_UNSUPPORTED');});
  test('numeric long IDs are never rounded into platform IDs',async()=>{await page.evaluate(()=>testConfig.numericId=true);assert.equal((await run(command())).code,'PAGE_UNSUPPORTED');});
  test('foreign scope and unknown filters are refused',async()=>{await page.evaluate(()=>{vm.ruleForm.institutionId='another';modal.ruleForm.institutionId='another';});assert.equal((await run(command())).code,'IDENTITY_CONFLICT');await reset();await page.evaluate(()=>vm.ruleForm.unknownFilter='restricted');assert.equal((await run(command())).code,'PAGE_UNSUPPORTED');});
  test('manual dialog and pending request prevent search',async()=>{await page.evaluate(()=>document.body.insertAdjacentHTML('beforeend','<div class="ant-modal">Manual edit</div>'));assert.equal((await run(command())).code,'REVIEW_REQUIRED');assert.equal(await page.evaluate(()=>searches.length),0);await reset();await page.evaluate(()=>vm.loading=true);assert.equal((await run(command())).code,'PAGE_TIMEOUT');});
  test('foreign host and expired commands are refused',async()=>{await page.goto('http://example.test/advancedSearch');assert.equal((await run(command())).code,'AUTH_REQUIRED');await reset();assert.equal((await run(command({expires:0}))).code,'PAGE_TIMEOUT');});
  test('front-end history route cannot be replaced by backend-style hash route',async()=>{await page.goto('http://www.ir.lib.sjtu.edu.cn/#/advancedSearch');assert.equal((await run(command())).code,'AUTH_REQUIRED');assert.equal(await page.evaluate(()=>searches.length),0);});
  for(const [name,fn] of cases){await reset();await fn();assert.equal(await page.evaluate(()=>listenerCount()),0,'temporary event listener must be removed');console.log('PASS '+name);}
  console.log(`Desktop library: ${cases.length} isolated UI contract checks passed; no live writes.`);
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
