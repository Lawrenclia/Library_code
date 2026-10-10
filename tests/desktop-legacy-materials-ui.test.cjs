const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
(async () => {
  const browser = await chromium.launch({executablePath:process.env.TEST_BROWSER||'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
  try {
    const page = await browser.newPage({viewport:{width:1260,height:860}});
    await page.addInitScript(() => {
      const batch = {root:'D:/isolated-legacy',roster_hash:'original-hash',review_required:true,records:[
        {kind:'classification',title:'已绑定历史论文',paper_id:'old-1',task_ids:['old-sa'],warnings:[],manifest:'classification.json'},
        {kind:'submission',title:'另一版本历史论文',paper_id:'old-2',task_ids:[],warnings:['名单版本不同，保留未绑定历史。'],manifest:'prepared.json'}
      ],files:[{original_file:'runtime/submission/原始导出/savedrecs.txt',archive_path:'D:/isolated-new/legacy-files/actual-hash.txt',sha256:'actual-file-hash',bytes:200,format:'txt'}],warnings:['旧准备断点尚未完成，仍需复核。']};
      window.testWorkspace = {tasks:[],root:'isolated-new',running:false,paused:false,browsers:{},policy:'PPT',legacy_materials:[]};
      window.testCommands=[];
      window.__TAURI_INTERNALS__={transformCallback:()=>1,unregisterCallback:()=>{},invoke:async(command,args)=>{
        window.testCommands.push({command,args});
        if(command==='workspace') return structuredClone(window.testWorkspace);
        if(command==='templates')return [];
        if(command==='ai_settings')return {base:'https://example.test/v1',model:'isolated',configured:false};
        if(command==='plugin:event|listen')return 1;
        if(command==='preview_legacy')return {root:batch.root,fingerprint:'exact-preview',roster_count:1,journal_count:0,import_count:0,orphan_count:0,classification_count:1,prepared_count:1,material_file_count:4,unbound_material_count:1,material_warnings:batch.warnings,entries:[{sa_id:'old-sa',title:'已绑定历史论文',histories:0,materials:1,phases:[],input_changed:false,needs_readback:false}]};
        if(command==='migrate_legacy'){window.testWorkspace.legacy_materials=[batch];return {count:1,already_imported:false};}
        if(command==='export_report')return {path:'isolated-report.xlsx'};
        return {};
      }};
    });
    await page.goto('http://127.0.0.1:1420');
    await page.getByRole('button',{name:'模型与设置',exact:true}).click();
    await page.getByRole('button',{name:'选择旧版目录并预览',exact:true}).click();
    await page.getByText('已绑定历史论文',{exact:false}).waitFor();
    assert((await page.textContent('body')).includes('1 条未绑定资料'));
    await page.getByText('查看迁移注意事项（1）',{exact:true}).click();
    await page.getByText('旧准备断点尚未完成，仍需复核。',{exact:true}).waitFor();
    await page.getByRole('button',{name:'迁移这些记录',exact:true}).click();
    await page.getByText('已迁入的历史材料',{exact:true}).waitFor();
    assert.deepEqual(await page.evaluate(()=>window.testCommands.find(c=>c.command==='migrate_legacy').args),{root:'D:/isolated-legacy',fingerprint:'exact-preview'});
    await page.getByText('批次 1 · 2 条记录 · 1 个文件',{exact:true}).click();
    await page.getByText('另一版本历史论文 · 提交准备',{exact:true}).waitFor();
    await page.getByText('未绑定 · 名单版本或对应关系待核对',{exact:true}).waitFor();
    await page.getByText('查看原文件、保存位置和哈希',{exact:true}).click();
    await page.getByText('D:/isolated-new/legacy-files/actual-hash.txt',{exact:true}).waitFor();
    assert.equal(await page.evaluate(()=>window.testCommands.some(c=>c.command==='run_step'||c.command==='classify')),false);
    const dir=path.resolve(__dirname,'../runtime/legacy-materials-preview');fs.mkdirSync(dir,{recursive:true});
    await page.screenshot({path:path.join(dir,'legacy-1260.png'),fullPage:true});
    await page.setViewportSize({width:960,height:680});
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    await page.screenshot({path:path.join(dir,'legacy-960.png'),fullPage:true});
    await page.reload();
    // Re-load the persisted IPC inventory; actual SQLite restart is a core test.
    await page.evaluate(()=>{window.testWorkspace.legacy_materials=[{root:'persisted-history',roster_hash:'original',review_required:true,records:[],files:[{original_file:'history.json',archive_path:'saved/history.json',sha256:'saved-hash',bytes:2,format:'json'}],warnings:[]}];});
    await page.getByRole('button',{name:'刷新工作台',exact:true}).click();
    await page.getByRole('button',{name:'模型与设置',exact:true}).click();
    await page.getByText('已迁入的历史材料',{exact:true}).waitFor();
    console.log('Legacy materials UI: material-only preview, counts, exact preview binding, incomplete/unbound review, original archive inventory, no platform/API actions and 1260/960 layout passed. SQLite restart is covered separately by core tests.');
  } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
