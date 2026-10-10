// Real Vue components with isolated IPC; no API or platform writes.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path');
(async () => {
  const browser = await chromium.launch({ executablePath: process.env.TEST_BROWSER || 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1260, height: 860 } });
    await page.addInitScript(() => {
      const task = (id, owner) => ({ id, revision: 0, record: {row:2, owner, sa_id:id, title:'材料整理测试论文', matches:0, staff_id:'001', doi:'', wos:'', mark:'待处理', reason:'原文', skipped:false, done:false}, route:'zero_review', stage:'pending', running:false, evidence:[], last_error:null, review:null, classification:null, artifact:null, platform_id:'', batch:null, sa_snapshot:null });
      window.work = { tasks:[task('s1', '原负责人'), task('s2', '其他负责人')], root:'isolated-materials', running:false, running_service:null, paused:false, browsers:{}, policy:'PPT' };
      window.commands=[]; window.callbacks={}; window.callbackId=0;
      window.emitChange=()=> { if(window.eventHandler) callbacks[eventHandler]({event:'workspace-changed',id:1,payload:{}}); };
      window.__TAURI_INTERNALS__={transformCallback:fn=> {callbacks[++callbackId]=fn;return callbackId;}, unregisterCallback:()=>{}, invoke:async(command,args)=> {
        commands.push({command,args});
        if(command==='workspace') return structuredClone(work);
        if(command==='ai_settings') return {configured:false,base:'',model:''};
        if(command==='templates') return [];
        if(command==='plugin:event|listen') {if(args.event==='workspace-changed') window.eventHandler=args.handler;return 1;}
        if(command==='prepare_material_batch') {
          work.material_batch={id:'original-material-batch',owner:args.owner,status:'running',cursor:0,targets:[{id:'s1'},{id:'s2'},{id:'s3'},{id:'s4'}],outcomes:[],pause_requested:false};
          work.running=true;work.running_service='materials';emitChange();
          await new Promise(resolve=>{window.releaseRun=resolve;});return {};
        }
        if(command==='pause_queue') {
          work.material_batch.status='paused';work.material_batch.cursor=1;work.material_batch.pause_requested=true;
          work.material_batch.outcomes=[{id:'s1',product:{kind:'original',path:'materials/products/raw.txt',audit:'source.json',reused:false,validation:{missing:[],ready:true}},error:null}];
          work.running=false;work.running_service=null;work.paused=true;
          if(window.releaseRun){releaseRun();window.releaseRun=null;}emitChange();return {};
        }
        if(command==='resume_material_batch') {
          work.material_batch.status='completed';work.material_batch.cursor=4;
          work.material_batch.outcomes.push({id:'s2',product:{kind:'draft',path:'materials/products/'+ 'a'.repeat(64)+'/模板草稿.xlsx',audit:'source.json',reused:true,validation:{missing:['作者单位'],invalid:[{column:'日期',reason:'日期无效'}],requires_review:[{column:'代码',reason:'动态代码待核对'}],ready:false}},error:null},
            {id:'s3',product:{kind:'field_valid',path:'materials/products/valid.xlsx',audit:'source.json',reused:false,validation:{missing:[],ready:true}},error:null},
            {id:'s4',product:null,error:{code:'EVIDENCE_REQUIRED',message:'缺少实际模板对应建议'}});
          emitChange();return {};
        }
        if(command==='cancel_material_batch') {work.material_batch.status='cancelled';emitChange();return {};}
        return {};
      }};
    });
    await page.goto('http://127.0.0.1:1420');
    const panel=page.getByRole('region',{name:'批量材料整理',exact:true});
    await page.getByLabel('负责人',{exact:true}).selectOption('原负责人');
    await panel.getByRole('button',{name:'整理当前负责人的零匹配资料',exact:true}).click();
    await panel.getByRole('button',{name:'暂停后续整理',exact:true}).waitFor();
    assert.deepEqual(await page.evaluate(()=>commands.find(c=>c.command==='prepare_material_batch').args),{owner:'原负责人'});
    await panel.getByRole('button',{name:'暂停后续整理',exact:true}).click();
    await panel.getByText(/已暂停 · 原范围/).waitFor();
    await page.getByLabel('负责人',{exact:true}).selectOption('其他负责人');
    assert.match(await panel.innerText(),/原范围：原负责人/);
    assert.equal(await panel.getByRole('button',{name:'整理当前负责人的零匹配资料',exact:true}).count(),0);
    await panel.getByRole('button',{name:'继续原材料范围',exact:true}).click();
    await panel.getByText(/本轮整理结束/).waitFor();
    assert.deepEqual(await page.evaluate(()=>commands.find(c=>c.command==='resume_material_batch').args),{id:'original-material-batch'});
    assert.match(await panel.innerText(),/原始导出 1 · 字段通过 1 · 待完善草稿 1 · 未生成 1/);
    await panel.locator('summary').click();
    assert.match(await panel.innerText(),/必填缺项：作者单位/);
    assert.match(await panel.innerText(),/日期无效/);
    assert.match(await panel.innerText(),/动态代码待核对/);
    assert.match(await panel.innerText(),/缺少实际模板对应建议/);
    await panel.getByRole('button',{name:'打开材料目录',exact:true}).click();
    assert.deepEqual(await page.evaluate(()=>commands.find(c=>c.command==='open_folder').args),{kind:'materials'});
    for(const width of [1260,960]) {
      await page.setViewportSize({width,height:860});
      assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
      const dir=path.resolve(__dirname,'../runtime/material-batch-preview');fs.mkdirSync(dir,{recursive:true});
      await page.screenshot({path:path.join(dir,`materials-${width}.png`),fullPage:true});
    }
    await page.evaluate(()=>{work.material_batch.status='running';work.running=true;work.running_service='ai';emitChange();});
    await panel.getByText(/正在整理 · 原范围/).waitFor();
    assert.equal(await panel.getByRole('button',{name:'暂停后续整理',exact:true}).count(),0);
    assert.equal(await panel.getByRole('button',{name:'继续原材料范围',exact:true}).isDisabled(),true);
    await page.evaluate(()=>{work.material_batch.status='interrupted';work.running=false;work.running_service=null;emitChange();});
    await panel.getByRole('button',{name:'结束原材料范围',exact:true}).click();
    await panel.getByText(/原范围已结束/).waitFor();
    assert.deepEqual(await page.evaluate(()=>commands.find(c=>c.command==='cancel_material_batch').args),{id:'original-material-batch'});
    assert.match(await panel.innerText(),/作者单位/);
    assert.equal(await page.evaluate(()=>commands.some(c=>['classify_task','run_ai_queue','run_step','fill_template'].includes(c.command))),false);
    assert.equal(await page.evaluate(()=>work.tasks[0].stage),'pending');
    console.log('Material batch UI passed: pending pause, frozen owner/scope, exact resume ID, original/draft/validated/failure states, missing/invalid/review details, managed folder, history retention, service-aware controls, 1260/960 layout. Isolated IPC only.');
  } finally {await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
