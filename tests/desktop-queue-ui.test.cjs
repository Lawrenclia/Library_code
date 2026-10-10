// Render the real Vue workbench with isolated IPC, no user profile or platform.
const {chromium}=require('playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path');
(async()=>{
  const browser=await chromium.launch({executablePath:process.env.TEST_BROWSER||'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
  try {
    const page=await browser.newPage({viewport:{width:960,height:740}});
    await page.addInitScript(()=>{
      const record={row:2,owner:'原负责人',sa_id:'original-sa',title:'本地队列恢复测试',doi:'',wos:'',staff_id:'001',matches:0,item_ids:'',mark:'待处理',reason:'本地测试',skipped:false,done:false,source:''};
      const task=(id,owner)=>({id,record:{...record,sa_id:id,owner},paper_id:'',revision:0,route:'zero_review',stage:'pending',running:false,evidence:[],last_error:null,review:null,classification:null,artifact:null,platform_id:'',batch:null,sa_snapshot:null});
      window.work={tasks:[task('original-sa','原负责人'),task('other-sa','其他负责人')],root:'本地隔离测试',running:false,paused:true,browsers:{},policy:'PPT',download_queue:{id:'original-queue',owner:'原负责人',retry_skipped:false,status:'interrupted',cursor:1,targets:[{id:'done-sa',fingerprint:'done',skipped:false},{id:'original-sa',fingerprint:'one',skipped:false},{id:'third-sa',fingerprint:'three',skipped:false}],outcomes:[{id:'done-sa',status:'review',error:{code:'NO_RESULT',message:'实际查询零条'},finished:1}],pause_requested:false,last_error:{code:'PAUSED',message:'重启后请继续原范围'}}};
      window.commands=[];
      window.__TAURI_INTERNALS__={transformCallback:()=>1,unregisterCallback:()=>{},invoke:async(command,args)=>{
        commands.push({command,args});
        if(command==='workspace')return structuredClone(work);
        if(command==='ai_settings')return {base:'https://example.org',model:'test',configured:false};
        if(command==='templates')return [];
        if(command==='plugin:event|listen')return 1;
        if(command==='resume_queue'){
          if(args.id!=='original-queue')throw Error('Original queue identity was lost');
          work.download_queue.status='blocked';work.download_queue.last_error={code:'BROWSER_DISCONNECTED',message:'请恢复原 WOS 工作页'};return {};
        }
        if(command==='cancel_queue') {work.download_queue.status='cancelled';return {};}
        if(command==='run_queue')return {};
        return {};
      }};
    });
    await page.goto('http://127.0.0.1:1420');
    const panel=page.getByRole('region',{name:'下载队列进度'});
    await panel.getByText('重启后待继续',{exact:true}).waitFor();
    assert.match(await panel.innerText(),/已记录 1 \/ 3 篇 · 剩余 2 篇/);
    assert.match(await panel.innerText(),/原负责人/);
    await page.getByLabel('负责人',{exact:true}).selectOption('其他负责人');
    assert.equal(await page.getByRole('button',{name:'检索 / 下载 WOS',exact:true}).isDisabled(),true,'Changing the owner cannot replace a pending queue');
    assert.equal(await page.getByRole('button',{name:'检索跳过项',exact:true}).isDisabled(),true);
    await panel.getByRole('button',{name:'继续原队列',exact:true}).click();
    await panel.getByText('工作页待恢复',{exact:true}).waitFor();
    assert.deepEqual(await page.evaluate(()=>commands.find(c=>c.command==='resume_queue').args),{id:'original-queue'});
    assert.match(await panel.innerText(),/请恢复原 WOS 工作页/);
    assert.match(await panel.innerText(),/已记录 1 \/ 3/);
    await panel.locator('summary').click();assert.match(await panel.innerText(),/done-sa · 待核验 · 实际查询零条/);
    await panel.getByRole('button',{name:'结束原范围',exact:true}).click();
    const dialog=page.getByRole('dialog');
    await dialog.getByText('结束原下载范围',{exact:true}).waitFor();
    assert.match(await dialog.innerText(),/原负责人/);
    await dialog.getByRole('button',{name:'返回核对',exact:true}).click();
    assert.equal(await page.evaluate(()=>commands.some(c=>c.command==='cancel_queue')),false,'Cancel confirmation must preserve pending progress');
    fs.mkdirSync(path.join(__dirname,'../runtime'),{recursive:true});
    await page.screenshot({path:path.join(__dirname,'../runtime/desktop-ui-queue.png'),fullPage:true});
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,'Compact queue card must fit the window');
    await panel.getByRole('button',{name:'结束原范围',exact:true}).click();
    await dialog.getByRole('button',{name:'确认执行',exact:true}).click();
    await panel.getByText('已结束原范围',{exact:true}).waitFor();
    assert.deepEqual(await page.evaluate(()=>commands.find(c=>c.command==='cancel_queue').args),{id:'original-queue'});
    assert.equal(await panel.getByRole('button',{name:'继续原队列',exact:true}).count(),0);
    assert.match(await panel.innerText(),/实际查询零条/,'Prior outcomes remain visible after cancelling');
    await page.getByRole('button',{name:'检索 / 下载 WOS',exact:true}).click();
    assert.deepEqual(await page.evaluate(()=>commands.find(c=>c.command==='run_queue').args),{owner:'其他负责人',retrySkipped:false});
    console.log('PASS queue UI: original owner/scope/progress survive restart; new scopes disabled; resume uses original queue ID; channel error retains cursor; cancel confirmation preserves outcomes; compact layout fits.');
  }finally{await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
