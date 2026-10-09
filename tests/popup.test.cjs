const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {chromium} = require('playwright');
(async () => {
  const edge='C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
  const executablePath=process.env.SA_TEST_BROWSER || (fs.existsSync(edge)?edge:undefined);
  const browser = await chromium.launch({headless:true,...(executablePath?{executablePath}:{})});
  try {
    const page = await browser.newPage({viewport:{width:400,height:600}});
    const base = path.join(__dirname,'../extension');
    await page.setContent(fs.readFileSync(path.join(base,'popup.html'),'utf8').replace(/<script[^>]*>.*?<\/script>/s,''));
    await page.addStyleTag({content:fs.readFileSync(path.join(base,'popup.css'),'utf8')});
    await page.evaluate(() => {
      window.calls=[];
      window.chrome={tabs:{query:async()=>[{id:1}]},runtime:{sendMessage:async message=>{
        calls.push(message.type);
        if(message.type==='popup_status') return {ok:true,data:{paired:false,version:'test'}};
        if(message.type==='pair') {
          await new Promise(r=>setTimeout(r,150));
          return {ok:true};
        }
        if(message.type==='inspect_workflow') return {ok:true,data:{site:'test',wos_error:false}};
        if(message.type==='disconnect') throw new Error('模拟断开失败');
        return {ok:true};
      }}};
    });
    await page.addScriptTag({content:fs.readFileSync(path.join(base,'popup.js'),'utf8')});
    await page.waitForFunction(()=>document.getElementById('connection').textContent.includes('尚未配对'));
    await page.locator('#pair').click();
    assert.match(await page.locator('#status').innerText(),/请先粘贴/);
    assert.equal(await page.evaluate(()=>calls.includes('pair')),false);
    await page.locator('#token').fill('a'.repeat(43));
    await page.locator('#token').press('Enter');
    await page.waitForFunction(()=>document.getElementById('pair').disabled);
    await page.evaluate(()=>document.getElementById('pair').click());
    await page.waitForFunction(()=>!document.getElementById('pair').disabled);
    assert.equal(await page.evaluate(()=>calls.filter(x=>x==='pair').length),1);
    assert.equal(await page.locator('#token').inputValue(),'');
    await page.locator('#inspect').click();
    await page.waitForFunction(()=>!document.getElementById('inspect').disabled);
    assert.equal(await page.locator('#copy-diagnostics').isVisible(),true);
    assert.match(await page.locator('#diagnostics').inputValue(),/test/);
    await page.locator('#disconnect').click();
    await page.waitForFunction(()=>!document.getElementById('disconnect').disabled);
    assert.match(await page.locator('#status').innerText(),/模拟断开失败/);
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=400),true);
    console.log('PASS popup empty token, Enter pairing, duplicate prevention, diagnostics, error recovery and width');
  } finally { await browser.close(); }
})().catch(error=>{console.error(error);process.exitCode=1;});
