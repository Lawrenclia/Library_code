/* Render only the project's local popup, never a user's browser profile. */
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const {pathToFileURL}=require('node:url');
const {chromium}=require('playwright');
const root=path.resolve(__dirname,'..');
(async()=>{
  const browser=await chromium.launch({headless:true,executablePath:process.env.SA_TEST_BROWSER||'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe'});
  try {
    const page=await browser.newPage({viewport:{width:400,height:600}});
    await page.addInitScript(()=>{
      window.calls=[];
      window.chrome={tabs:{query:async()=>[{id:1}]},runtime:{sendMessage:async message=>{
        window.calls.push(message);return message.type==='popup_status' ? {ok:true,data:{paired:false,version:'test'}} : {ok:true};}}};
    });
    await page.goto(pathToFileURL(path.join(root,'extension/popup.html')).href);
    const folder=path.join(root,'runtime/ui-preview');fs.mkdirSync(folder,{recursive:true});
    await page.screenshot({path:path.join(folder,'popup-0.3.25.png')});
    assert.equal(await page.locator('.workflow').evaluate(el=>el.open),false);
    const status=await page.locator('#status').boundingBox();
    assert.ok(status.y+status.height<=600,'connection status must be visible on opening');
    await page.locator('summary').click();
    const labels={pair:'连接此页与桌面助手',wos:'将此页用于 WOS 检索',import:'将此页用于 TXT 入库',
      'open-import':'打开数据导入与批次管理',inspect:'查看连接诊断',mute:'WOS 静音 / 恢复',disconnect:'断开连接'};
    for (const [id,text] of Object.entries(labels)) {
      const button=page.locator('#'+id);
      assert.ok((await button.innerText()).includes(text));
      await button.scrollIntoViewIfNeeded();
      const dimensions=await button.evaluate(el=>({width:el.clientWidth,scroll:el.scrollWidth,rect:el.getBoundingClientRect().toJSON()}));
      assert.ok(dimensions.scroll<=dimensions.width+1,`${id} text overflows`);
      assert.ok(dimensions.rect.x>=0&&dimensions.rect.right<=400,`${id} outside popup`);
    }
    await page.locator('#token').fill('synthetic-pair-code');
    await page.locator('#pair').click();
    assert.equal(await page.locator('#token').inputValue(),'');
    await page.locator('#wos').click();
    assert.ok((await page.locator('#status').innerText()).includes('用于 WOS 检索'));
    await page.locator('#import').click();
    assert.ok((await page.locator('#status').innerText()).includes('用于 TXT 入库'));
    const calls=await page.evaluate(()=>window.calls);
    const actions=calls.filter(value=>value.type!=='popup_status');
    assert.deepEqual(actions.map(value=>value.type),['pair','bind_workflow','bind_workflow']);
    assert.deepEqual(actions.slice(1).map(value=>value.role),['wosTabId','importTabId']);
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
    await page.locator('#status').evaluate(el=>{el.textContent='长连接诊断示例 '.repeat(100);});
    const card=await page.locator('.status-card').boundingBox();
    assert.ok(card.height<180,'long status must not squeeze the other controls');
    await page.locator('#diagnostics').evaluate(el=>{el.hidden=false;el.value='synthetic-diagnostic-'.repeat(100);});
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
    await page.locator('#status').evaluate(el=>{el.textContent='布局检查完成';});
    await page.locator('#diagnostics').evaluate(el=>{el.hidden=true;});
    await page.evaluate(()=>window.scrollTo(0,0));
    await page.screenshot({path:path.join(folder,'popup-0.3.25-expanded.png'),fullPage:true});
    console.log('PASS popup: collapsed/expanded layout and long diagnostics fit; 7 actions and WOS/import roles unchanged');
  } finally {await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
