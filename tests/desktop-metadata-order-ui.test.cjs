// Actual Vue UI with isolated IPC; no institution website or real writes.
const {chromium}=require('playwright'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
(async()=>{
 const browser=await chromium.launch({executablePath:process.env.TEST_BROWSER||'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
 try{
  const page=await browser.newPage({viewport:{width:960,height:760}});
  await page.addInitScript(()=>{
   const record={row:2,owner:'测试负责人',sa_id:'order-ui',title:'完整作者与单位顺序核验',doi:'10.1234/test',wos:'',staff_id:'00001',matches:1,item_ids:'item1',mark:'待处理',reason:'第一作者；交大第一单位',skipped:false,done:false,source:'本地模拟'};
   window.testWorkspace={tasks:[{id:record.sa_id,paper_id:'paper1',revision:0,record,route:'existing',stage:'awaiting_review',running:false,evidence:[],last_error:null,review:null,classification:null,artifact:null,platform_id:'item1',batch:null,sa_snapshot:null,issue_plan:null,issue_reviews:[]}],root:'本地隔离目录',running:false,paused:false,browsers:{},policy:'PPT'};
   window.testCommands=[];
   const checklist=()=>({requirements:[{key:'first_author',label:'第一作者标记不一致',sa:'是',library_before:'否',library:'否'},
     {key:'first_institution',label:'交大是否第一单位不一致',sa:'是',library_before:'否',library:'否'}],baseline:{row:{}},live:{row:{saLzkId:'order-ui'}}});
   window.__TAURI_INTERNALS__={transformCallback:()=>1,unregisterCallback:()=>{},invoke:async(command,args)=>{
    window.testCommands.push({command,args});
    if(command==='workspace')return structuredClone(window.testWorkspace);
    if(command==='ai_settings')return {base:'https://example.invalid/v1',model:'test',configured:false};
    if(command==='templates')return [];
    if(command==='plugin:event|listen')return 1;
    if(command==='run_step'&&args.action==='prepare_issues'){
     const plan=checklist();window.testWorkspace.tasks[0].issue_plan=structuredClone(plan);return plan;
    }
    if(command==='run_step'&&args.action==='prepare_metadata')return {sa:{},identity:{},names:['Tester'],result:{item_id:'item1',staff_id:'00001',scholar:{id:'scholar1',wno:'00001',nameCn:'测试学者'},
     authors:[{index:0,id:'author0',fullname:'Other',order:1,eligible:false,fields:[]},{index:1,id:'author1',fullname:'Tester',order:2,eligible:true,fields:['commonFirst']}],
     institutions:[{index:0,order:1,address:'Synthetic University',first_institution_value:'否'},{index:1,order:2,address:'Shanghai Jiao Tong University',first_institution_value:'是'}],
     can_reorder_authors:true,can_reorder_institutions:!window.noUnitControl,snapshot:{}}};
    if(command==='run_step'&&args.action==='save_metadata'){
     window.testWorkspace.tasks[0].stage='unknown';window.testWorkspace.tasks[0].pending_action='save_metadata';
     throw {code:'REMOTE_RESULT_UNKNOWN',message:'模拟结果未确认，需要完整回读。'};
    }
    if(command==='run_step'&&args.action==='verify_metadata'){
     window.testWorkspace.tasks[0].stage='awaiting_review';window.testWorkspace.tasks[0].pending_action=null;return {verified:true};
    }
    return {};
   }};
  });
  await page.goto('http://127.0.0.1:1420');
  await page.getByText('完整作者与单位顺序核验',{exact:true}).click();
  await page.getByRole('tab',{name:'核验',exact:true}).click();
  await page.getByRole('button',{name:'读取 / 刷新逐项核对清单',exact:true}).click();
  const reason=page.getByLabel('本次核对的原因',{exact:true}),operation=page.getByLabel('本项修改方式',{exact:true}),author=page.getByLabel('本库待修改的作者行',{exact:true});
  const submit=page.getByRole('button',{name:'确认保存本项修改并回读',exact:true});
  const writes=()=>page.evaluate(()=>window.testCommands.filter(c=>c.command==='run_step'&&c.args.action==='save_metadata'));
  const prepare=async key=>{await reason.selectOption(key);await page.getByLabel('本项处理结论',{exact:true}).selectOption('sa_correct');await page.getByRole('button',{name:'按工号读取可编辑作者字段',exact:true}).click();};
  const evidence=async()=>{await page.getByLabel('本项原文或来源',{exact:true}).fill('本地原文 PDF');await page.getByLabel('本项具体依据',{exact:true}).fill('原文的完整作者及单位顺序');await page.getByLabel('本项核对备注',{exact:true}).fill('逐项核对原始身份及编号');};
  await prepare('first_author');
  assert.equal(await operation.locator('option[value="role"]').textContent(),'共同第一作者标记');
  await operation.selectOption('author_order');assert.equal(await author.evaluate(el=>el.selectedOptions[0]._value),null,'选择修改方式不会替用户选择作者身份');
  await author.selectOption('1');await evidence();assert.equal(await submit.isDisabled(),true,'原顺序没有变化，不允许重复保存');
  const order=page.getByRole('list',{name:'修改后的完整顺序',exact:true});
  await page.getByRole('button',{name:'上移 Tester',exact:true}).click();assert.match(await order.innerText(),/^1\. Tester/);
  assert.equal(await page.getByRole('button',{name:'上移 Tester',exact:true}).isDisabled(),true,'首行无法继续上移');
  await submit.click();await page.getByRole('dialog').getByText(/作者 ID：author1/).waitFor();
  assert.match(await page.getByRole('dialog').innerText(),/新顺序：\s*1\. Tester\s*2\. Other/);
  await page.getByRole('button',{name:'返回核对',exact:true}).click();assert.equal((await writes()).length,0,'取消确认不提交');
  await submit.click();await page.getByLabel('本项具体依据',{exact:true}).evaluate(el=>{el.value='确认后改变的依据';el.dispatchEvent(new Event('input',{bubbles:true}));});
  await page.getByRole('button',{name:'确认执行',exact:true}).click();await page.getByText('编辑对象或依据已变化，请重新核对。',{exact:true}).waitFor();
  assert.equal((await writes()).length,0,'确认期间来源变化不得使用旧计划');
  await evidence();await submit.click();await page.getByRole('button',{name:'确认执行',exact:true}).click();
  await page.getByRole('button',{name:'核验上次本库字段保存结果',exact:true}).waitFor();
  assert.deepEqual((await writes())[0].args.extra,{key:'first_author',operation:'author_order',order:[1,0],author_index:1,source:'本地原文 PDF',proof:'原文的完整作者及单位顺序',note:'逐项核对原始身份及编号'});
  assert.equal(await submit.isDisabled(),true,'未知结果禁止重复写入');
  await page.getByRole('button',{name:'核验上次本库字段保存结果',exact:true}).click();assert.equal(await operation.count(),0,'只读核验完成后清空旧排序表单');
  await prepare('first_institution');assert.equal(await operation.inputValue(),'institution_order');assert.equal(await author.evaluate(el=>el.selectedOptions[0]._value),null);
  assert.equal(await page.getByLabel('本项原文或来源',{exact:true}).inputValue(),'','各原因独立取证');
  await author.selectOption('1');await evidence();assert.equal(await submit.isDisabled(),true);
  await page.getByRole('button',{name:'上移 Shanghai Jiao Tong University',exact:true}).click();
  assert.match(await order.innerText(),/^1\. Shanghai Jiao Tong University/);assert.equal(await submit.isDisabled(),false);
  await order.scrollIntoViewIfNeeded();fs.mkdirSync(path.resolve(__dirname,'../runtime'),{recursive:true});
  await page.screenshot({path:path.resolve(__dirname,'../runtime/desktop-ui-metadata-order.png'),fullPage:true});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,'最小窗口无横向溢出');
  await submit.click();assert.match(await page.getByRole('dialog').innerText(),/同步作者单位编号/);
  await page.getByRole('button',{name:'确认执行',exact:true}).click();await page.getByRole('button',{name:'核验上次本库字段保存结果',exact:true}).waitFor();
  assert.deepEqual((await writes())[1].args.extra,{key:'first_institution',operation:'institution_order',order:[1,0],author_index:1,source:'本地原文 PDF',proof:'原文的完整作者及单位顺序',note:'逐项核对原始身份及编号'});
  await page.getByRole('button',{name:'核验上次本库字段保存结果',exact:true}).click();
  await page.evaluate(()=>window.noUnitControl=true);await prepare('first_institution');
  assert.equal(await author.locator('option').count(),1,'缺少实际可编辑单位条件，不提供作者或保存入口');
  assert.equal(await submit.isDisabled(),true);
  await reason.selectOption('first_author');assert.equal(await operation.count(),0,'切换原因清空整个旧排序草稿');
  await page.getByLabel('本项处理结论',{exact:true}).selectOption('sa_correct');await page.getByRole('button',{name:'按工号读取可编辑作者字段',exact:true}).click();
  await operation.selectOption('author_order');assert.match(await order.innerText(),/^1\. Other/,'新准备显示原表完整顺序，不沿用上一次调整');
  console.log('Metadata order UI: distinct role/order modes, exact full preview, explicit identity/source, no-op disabled, cancel/stale-confirm blocked, immutable order payload, unknown readback, clean issue reset and 960px layout passed; isolated IPC only.');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
