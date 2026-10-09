const {chromium}=require('playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const {runMetadataCommand}=require('../desktop/src-tauri/browser/metadata-adapter.cjs');
(async()=>{
 const browser=await chromium.launch({executablePath:process.env.TEST_BROWSER||'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
 try{
  const context=await browser.newContext();
  const fixture=fs.readFileSync(path.join(__dirname,'fixtures/metadata.html'),'utf8');
  await context.route('**/*',route=>route.fulfill({status:200,contentType:'text/html',body:fixture}));
  const page=await context.newPage();
  const reset=async()=>{await page.goto('http://admin.ir.lib.sjtu.edu.cn/#/dataCompare/list');await page.reload();};
  const cmd=(action,extra={})=>({action,sa_id:'sa-001',item_id:'item-001',staff_id:'00001',scholar:{id:'scholar-001',wno:'00001'},names:['Demo, X'],expected_row:{saLzkId:'sa-001',gh:'00001',itemId:'item-001',matchCount:1,markStatus:'待处理',titleValue:'A synthetic paper'},expires:Date.now()+45000,...extra});
  const run=command=>page.evaluate(runMetadataCommand,command);
  const prepare=async()=>{const result=await run(cmd('metadata_read'));assert.equal(result.ok,true,JSON.stringify(result));return result.data;};
  const save=(prepared,extra={})=>run(cmd('metadata_save',{author_index:1,author_id:'author-002',fullname:'Demo, X',key:'corresponding_author',value:true,confirmed:true,expected_snapshot:prepared.snapshot,...extra}));
  const orderExtra=(prepared,operation)=>{
    const wanted=structuredClone(prepared.snapshot.form);
    const field=operation==='author_order'?'author':'authorInstitution';
    wanted.metadata[field].reverse().forEach((row,index)=>row.order=index+1);
    if(operation==='institution_order'){
      wanted.metadata.author[0].institutionOrderNums='2';
      wanted.metadata.author[1].institutionOrderNums='2,1';
    }
    return {operation,order:[1,0],key:operation==='author_order'?'first_author':'first_institution',expected_form:wanted};
  };
  const cases=[];const test=(name,fn)=>cases.push([name,fn]);
  test('reads full metadata with exact staff and scholar, no save or credential access',async()=>{const p=await prepare();assert.equal(p.authors[0].eligible,false);assert.equal(p.authors[1].eligible,true);assert.deepEqual(p.authors[1].fields,['correspondent','commonFirst']);assert.equal(p.snapshot.form.metadata.abstract[0],'Important original content');assert.equal(await page.evaluate(()=>metadataWrites),0);});
  test('corresponding author checkbox saves once, awaits model reload and preserves every unrelated field',async()=>{const p=await prepare();await page.evaluate(()=>testConfig.modelDelay=80);const r=await save(p);assert.equal(r.ok,true,JSON.stringify(r));assert.equal(r.data.verified,true);assert.equal(r.data.after.metadata.author[1].correspondent,true);assert.equal(r.data.after.metadata.pages[0],'100-108');assert.deepEqual(r.data.after.fullTexts,p.snapshot.form.fullTexts);assert.equal(await page.evaluate(()=>metadataWrites),1);});
  test('only visible common-first binding changes; natural author order stays intact',async()=>{const p=await prepare();const r=await save(p,{key:'first_author'});assert.equal(r.ok,true,JSON.stringify(r));assert.equal(r.data.after.metadata.author[1].commonFirst,true);assert.equal(r.data.after.metadata.author[1].order,2);assert.equal(r.data.after.metadata.author[0].order,1);assert.equal(r.data.after.metadata.author[0].commonFirst,false);});
  test('stale repeat cannot submit a second time',async()=>{const p=await prepare();assert.equal((await save(p)).ok,true);assert.equal((await save(p)).code,'TASK_CHANGED');assert.equal(await page.evaluate(()=>metadataWrites),1);});
  test('missing confirmation and mismatched author ID never save',async()=>{const p=await prepare();assert.equal((await save(p,{confirmed:false})).submitted,false);assert.equal((await save(p,{author_id:'author-001'})).code,'IDENTITY_CONFLICT');assert.equal(await page.evaluate(()=>metadataWrites),0);});
  test('full form drift blocks save and automatic close without discarding a draft',async()=>{const p=await prepare();await page.evaluate(()=>editor.ruleForm.metadata.pages=['human draft']);assert.equal((await save(p)).code,'REVIEW_REQUIRED');assert.equal((await run(cmd('metadata_close'))).code,'REVIEW_REQUIRED');assert.equal(await page.evaluate(()=>editor.ruleForm.metadata.pages[0]),'human draft');assert.equal(await page.evaluate(()=>metadataWrites),0);});
  test('source and publication drift after save remains unknown, no success claim',async()=>{const p=await prepare();await page.evaluate(()=>testConfig.mutateOther=true);const r=await save(p);assert.equal(r.ok,false);assert.equal(r.code,'REMOTE_RESULT_UNKNOWN');assert.equal(r.submitted,true);assert.equal(await page.evaluate(()=>metadataWrites),1);});
  test('SA row change, foreign modal and manual editor ownership stop before save',async()=>{const p=await prepare();await page.evaluate(()=>compare.tableData[0].gh='00002');assert.equal((await save(p)).code,'IDENTITY_CONFLICT');await reset();await page.evaluate(()=>delete editOwner.__libraryTask);assert.equal((await run(cmd('metadata_read'))).code,'REVIEW_REQUIRED');await reset();await page.evaluate(()=>document.body.insertAdjacentHTML('beforeend','<div class="el-dialog">Foreign draft</div>'));assert.equal((await run(cmd('metadata_read'))).code,'REVIEW_REQUIRED');});
  test('conflicting scholar association cannot be selected even with matching fullname',async()=>{await page.evaluate(()=>{editor.ruleForm.metadata.author[1].scholarId='another-scholar';render();});const p=await prepare();assert.equal(p.authors[1].eligible,false);assert.equal((await save(p)).code,'IDENTITY_CONFLICT');assert.equal(await page.evaluate(()=>metadataWrites),0);});
  test('repeated fullname is not automatically mapped to a role checkbox',async()=>{await page.evaluate(()=>{editor.ruleForm.metadata.author[0].fullname='Demo, X';render();});const p=await prepare();assert.deepEqual(p.authors[1].fields,[]);assert.equal((await save(p)).code,'PAGE_UNSUPPORTED');assert.equal(await page.evaluate(()=>metadataWrites),0);});
  test('disabled, hidden or absent controls are not written through hidden data',async()=>{for(const key of ['disabled','hiddenCheckbox','missingCheckbox','readOnly']){await reset();await page.evaluate(key=>{testConfig[key]=true;editor.modelFieldList=schema();render();},key);const p=await prepare();assert.deepEqual(p.authors[1].fields,[]);assert.equal((await save(p)).ok,false);assert.equal(await page.evaluate(()=>metadataWrites),0);}});
  test('own or common corresponding flag prevents false demotion through another checkbox',async()=>{await page.evaluate(()=>{editor.ruleForm.metadata.author[1].ownCorrespondent=true;editor.ruleForm.metadata.author[1].correspondent=true;render();});const p=await prepare();assert.equal((await save(p,{value:false})).code,'REVIEW_REQUIRED');assert.equal(await page.evaluate(()=>metadataWrites),0);});
  test('first author by natural order cannot be cleared using common-first checkbox',async()=>{await page.evaluate(()=>{editor.ruleForm.metadata.author[1].order=1;editor.ruleForm.metadata.author[1].commonFirst=true;render();});const p=await prepare();assert.equal((await save(p,{key:'first_author',value:false})).code,'REVIEW_REQUIRED');assert.equal(await page.evaluate(()=>metadataWrites),0);});
  test('platform validation failure restores the checkbox and records not-submitted',async()=>{const p=await prepare();await page.evaluate(()=>testConfig.validationFails=true);const r=await save(p);assert.equal(r.code,'INCOMPLETE_METADATA');assert.equal(r.submitted,false);assert.equal(await page.evaluate(()=>editor.ruleForm.metadata.author[1].correspondent),false);assert.equal(await page.evaluate(()=>metadataWrites),0);});
  test('lost result is submitted once; read-only recovery obtains persisted full metadata',async()=>{const p=await prepare();await page.evaluate(()=>testConfig.hangSave=true);const r=await save(p,{expires:Date.now()+4200});assert.equal(r.code,'REMOTE_RESULT_UNKNOWN');assert.equal(r.submitted,true);assert.equal(await page.evaluate(()=>metadataWrites),1);
    await page.evaluate(()=>{persistedForm=copy(editor.ruleForm);editor.loading=false;testConfig.hangSave=false;});
    const recovered=await run(cmd('metadata_check',{author_index:1,author_id:'author-002',fullname:'Demo, X',key:'corresponding_author',value:true,expected_snapshot:p.snapshot}));assert.equal(recovered.ok,true,JSON.stringify(recovered));assert.equal(recovered.data.verified,true);assert.equal(await page.evaluate(()=>metadataWrites),1);
  });
  test('absence of persisted modification cannot authorize retry',async()=>{const p=await prepare();const r=await run(cmd('metadata_check',{author_index:1,author_id:'author-002',fullname:'Demo, X',key:'corresponding_author',value:true,expected_snapshot:p.snapshot}));assert.equal(r.code,'REMOTE_RESULT_UNKNOWN');assert.equal(await page.evaluate(()=>metadataWrites),0);});
  test('clean owned editor may close; manual changes and in-flight save may not',async()=>{await prepare();assert.equal((await run(cmd('metadata_close'))).data.closed,true);await reset();await prepare();await page.evaluate(()=>editor.loading=true);const r=await run(cmd('metadata_close',{expires:Date.now()+4200}));assert.equal(r.ok,false);assert.equal(await page.evaluate(()=>editOwner.drawer),true);});
  test('institution order, numeric IDs, foreign hosts and expired commands are refused',async()=>{const p=await prepare();assert.equal((await save(p,{key:'first_institution'})).code,'PAGE_UNSUPPORTED');await page.evaluate(()=>editor.ruleForm.metadata.author[1].id=123);assert.equal((await run(cmd('metadata_read'))).code,'IDENTITY_CONFLICT');await page.goto('http://example.invalid/#/dataCompare/list');assert.equal((await run(cmd('metadata_read'))).code,'AUTH_REQUIRED');await reset();assert.equal((await run(cmd('metadata_read',{expires:0}))).code,'PAGE_TIMEOUT');});
  test('complete author reorder saves original IDs once and preserves all original roles, units and content',async()=>{
    const p=await prepare();assert.equal(p.can_reorder_authors,true);const extra=orderExtra(p,'author_order');
    const r=await save(p,extra);assert.equal(r.ok,true,JSON.stringify(r));assert.equal(r.data.operation,'author_order');
    assert.deepEqual(r.data.after.metadata.author.map(a=>[a.id,a.order]),[['author-002',1],['author-001',2]]);
    assert.equal(r.data.after.metadata.author[0].commonFirst,false);assert.equal(r.data.after.metadata.author[0].institutionOrderNums,'1,2');
    assert.deepEqual(r.data.after.metadata.authorInstitution,p.snapshot.form.metadata.authorInstitution);
    assert.deepEqual(r.data.after.fullTexts,p.snapshot.form.fullTexts);assert.equal(r.data.after.metadata.abstract[0],'Important original content');
    assert.equal(await page.evaluate(()=>metadataWrites),1);assert.equal((await save(p,extra)).submitted,false);assert.equal(await page.evaluate(()=>metadataWrites),1);
  });
  test('complete institution reorder keeps entities and remaps every author affiliation before full reload',async()=>{
    const p=await prepare();assert.equal(p.can_reorder_institutions,true);assert.deepEqual(p.institutions.map(r=>r.first_institution_value),['否','是']);
    const r=await save(p,orderExtra(p,'institution_order'));assert.equal(r.ok,true,JSON.stringify(r));
    assert.deepEqual(r.data.after.metadata.authorInstitution.map(r=>[r.id,r.order]),[['institution-2',1],['institution-1',2]]);
    assert.deepEqual(r.data.after.metadata.author.map(a=>[a.id,a.order,a.institutionOrderNums]),[['author-001',1,'2'],['author-002',2,'2,1']]);
    assert.equal(r.data.after.metadata.author[1].scholarId,'scholar-001');assert.equal(await page.evaluate(()=>metadataWrites),1);
  });
  test('missing, duplicated, unchanged or forged order never reaches submitData',async()=>{
    const p=await prepare(),extra=orderExtra(p,'author_order');
    for(const order of [[0,1],[1],[1,1],[1,2],[1,'0'],[1,0,2]])assert.equal((await save(p,{...extra,order})).submitted,false);
    const forged=structuredClone(extra);forged.expected_form.metadata.pages=[];
    assert.equal((await save(p,forged)).code,'TASK_CHANGED');assert.equal((await save(p,{...extra,author_id:'missing'})).code,'IDENTITY_CONFLICT');
    assert.equal(await page.evaluate(()=>metadataWrites),0);
  });
  test('hidden or readonly complete rows and absent institution helper cannot authorize order edits',async()=>{
    for(const config of ['readOnlyName','hiddenInstitution','disabledInstitution','readOnlyInstitution','noHelper']){
      await reset();await page.evaluate(config=>{testConfig[config]=true;if(config==='noHelper')delete compare.$refs.compareDetailDrawer.getKmsTopInstitution;editor.modelFieldList=schema();render();},config);
      const p=await prepare(),operation=config==='readOnlyName'?'author_order':'institution_order';
      assert.equal(p[operation==='author_order'?'can_reorder_authors':'can_reorder_institutions'],false,config);
      assert.equal((await save(p,orderExtra(p,operation))).ok,false);assert.equal(await page.evaluate(()=>metadataWrites),0);
    }
  });
  test('malformed original affiliations and wrong source business value block unit reordering',async()=>{
    for(const refs of ['','01','1,1','1,3',' 1']){
      await reset();await page.evaluate(refs=>{editor.ruleForm.metadata.author[0].institutionOrderNums=refs;render();},refs);
      const p=await prepare();assert.equal((await save(p,orderExtra(p,'institution_order'))).code,'PAGE_UNSUPPORTED');assert.equal(await page.evaluate(()=>metadataWrites),0);
    }
    await reset();const p=await prepare();assert.equal((await save(p,{...orderExtra(p,'institution_order'),value:false})).code,'REVIEW_REQUIRED');
    assert.equal(await page.evaluate(()=>metadataWrites),0);
  });
  test('author reorder cannot silently clear common or own first flags when source expects false',async()=>{
    await page.evaluate(()=>{editor.ruleForm.metadata.author[1].ownFirst=true;render();});const p=await prepare();
    assert.equal((await save(p,{...orderExtra(p,'author_order'),value:false})).code,'REVIEW_REQUIRED');assert.equal(await page.evaluate(()=>metadataWrites),0);
    assert.equal(await page.evaluate(()=>editor.ruleForm.metadata.author[1].ownFirst),true);
  });
  test('platform validation restores entire order and all affiliation links without submitting',async()=>{
    const p=await prepare();await page.evaluate(()=>testConfig.validationFails=true);
    const r=await save(p,orderExtra(p,'institution_order'));assert.equal(r.code,'INCOMPLETE_METADATA');assert.equal(r.submitted,false);
    assert.deepEqual(await page.evaluate(()=>editor.ruleForm),p.snapshot.form);assert.equal(await page.evaluate(()=>metadataWrites),0);
  });
  test('lost author reorder result recovers by original author ID despite changed position, without resubmission',async()=>{
    const p=await prepare(),extra=orderExtra(p,'author_order');await page.evaluate(()=>testConfig.hangSave=true);
    const r=await save(p,{...extra,expires:Date.now()+4200});assert.equal(r.code,'REMOTE_RESULT_UNKNOWN');assert.equal(r.submitted,true);
    await page.evaluate(()=>{persistedForm=copy(editor.ruleForm);editor.loading=false;testConfig.hangSave=false;});
    const recovered=await run(cmd('metadata_check',{...extra,author_index:1,author_id:'author-002',fullname:'Demo, X',value:true,expected_snapshot:p.snapshot}));
    assert.equal(recovered.ok,true,JSON.stringify(recovered));assert.equal(recovered.data.after.metadata.author[0].id,'author-002');assert.equal(await page.evaluate(()=>metadataWrites),1);
  });
  test('partial institution save or changed unrelated content remains unknown after the one write',async()=>{
    const p=await prepare();await page.evaluate(()=>testConfig.mutateOther=true);
    const r=await save(p,orderExtra(p,'institution_order'));assert.equal(r.code,'REMOTE_RESULT_UNKNOWN');assert.equal(r.submitted,true);assert.equal(await page.evaluate(()=>metadataWrites),1);
  });
  for(const [name,fn]of cases){await reset();await fn();console.log('PASS '+name);}
  console.log(`Desktop metadata: ${cases.length} isolated author/institution editor contracts passed; no live writes.`);
 }finally{await browser.close();}
})().catch(error=>{console.error(error);process.exit(1);});
