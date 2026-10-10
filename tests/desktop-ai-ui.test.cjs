const {chromium}=require('playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
(async()=>{
 const browser=await chromium.launch({executablePath:process.env.TEST_BROWSER||'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
 try {
  const page=await browser.newPage({viewport:{width:1260,height:860}});
  await page.addInitScript(()=>{
   const record={row:2,owner:'分类测试',sa_id:'ai-task',title:'名单分类论文',doi:'',wos:'',staff_id:'001',matches:0,item_ids:'',mark:'待处理',reason:'',skipped:false,done:false,source:''};
   window.testWorkspace={tasks:[{id:'ai-task',paper_id:'paper',revision:0,record,route:'zero_review',stage:'pending',running:false,evidence:[],last_error:null,review:null,classification:null,artifact:null,platform_id:'',batch:null,sa_snapshot:null}],root:'isolated-ai',running:false,paused:false,browsers:{},policy:'PPT'};
   window.testCommands=[];
   window.__TAURI_INTERNALS__={transformCallback:()=>1,unregisterCallback:()=>{},invoke:async(command,args)=>{
    window.testCommands.push({command,args});
    if(command==='workspace')return structuredClone(window.testWorkspace);
    if(command==='ai_settings')return {base:'https://example.test/v1',model:'isolated',configured:true};
    if(command==='templates')return [{id:'template',name:'测试模板',columns:['Title'],required:['Title'],notes:''}];
    if(command==='plugin:event|listen')return 1;
    if(command==='classify_task') {
     if(window.failAi)throw {code:'AI_RESULT_INVALID',message:'AI 输出未正常完成，未采纳。'};
     const task=window.testWorkspace.tasks[0];
     task.classification={type:null,channel:null,confidence:'低',reason:'仅有题名，需核实成果类型。',channel_reason:'补查数据库并确认可用导出。',evidence_ids:['roster-current'],missing:['出版来源与署名'],fields:args.template?{Title:{value:task.record.title,evidence_ids:['roster-current']}}:{},template_id:args.template?.id||'',review_required:true,platform_verified:false};
     task.revision++;task.evidence.push({id:'audit',kind:'ai_classification',source:'API 模型：isolated',text:JSON.stringify({schema:'ai_classification_v2',result:task.classification,sources:[{id:'roster-current',kind:'roster_input',text:'原始名单'}],review_required:true,platform_verified:false}),created:1});
     return structuredClone(task.classification);
    }
    return {};
   }};
  });
  await page.goto('http://127.0.0.1:1420');
  await page.getByText('名单分类论文',{exact:true}).first().click();
  await page.getByRole('tab',{name:'AI 建议',exact:true}).click();
  const classify=page.getByRole('button',{name:'调用 API 分类 / 填写',exact:true});
  assert.equal(await classify.isDisabled(),false,'A configured model can classify the roster before external sources exist.');
  await classify.click();await page.getByText('成果类型待判定',{exact:true}).waitFor();
  await page.getByText('模型置信度：低 · 建议待复核',{exact:true}).waitFor();
  await page.getByText('仍需补充：出版来源与署名',{exact:true}).waitFor();
  assert((await page.textContent('body')).includes('待核实收录和文件'));
  assert.equal(await page.evaluate(()=>window.testWorkspace.tasks[0].stage),'pending');
  await page.getByLabel('填写模板',{exact:true}).selectOption('template');await classify.click();
  await page.getByText('名单分类论文',{exact:true}).last().waitFor();
  assert.equal(await page.evaluate(()=>window.testCommands.filter(c=>c.command==='classify_task').at(-1).args.template.id),'template');
  await page.evaluate(()=>window.failAi=true);await classify.click();await page.getByText(/AI 输出未正常完成，未采纳/).waitFor();
  await page.getByText('模型置信度：低 · 建议待复核',{exact:true}).waitFor();
  assert.equal(await page.evaluate(()=>window.testCommands.some(c=>c.command==='run_step'||c.command==='fill_template')),false);
  const dir=path.resolve(__dirname,'../runtime/ai-preview');fs.mkdirSync(dir,{recursive:true});
  await page.screenshot({path:path.join(dir,'ai-1260.png'),fullPage:true});await page.setViewportSize({width:960,height:680});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));await page.screenshot({path:path.join(dir,'ai-960.png'),fullPage:true});
  await page.getByRole('tab',{name:'来源',exact:true}).click();await page.getByText('AI 分类记录 · 查看完整建议与来源',{exact:true}).first().click();
  assert((await page.textContent('body')).includes('ai_classification_v2'));
  console.log('AI UI: roster-only entry, unknown type/channel, low confidence, missing evidence, template selection, rejected-response retention, complete audit, unchanged stage and 1260/960 layout passed. Isolated IPC; no API charge or platform write.');
 } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
