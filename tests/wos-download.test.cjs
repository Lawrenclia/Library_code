// Exercise the real dispatcher and page adapter together. All WOS traffic is
// fulfilled locally; Chrome's download API is represented by Playwright events.
const assert=require('node:assert/strict');
const fs=require('node:fs');
const os=require('node:os');
const path=require('node:path');
const vm=require('node:vm');
const {chromium}=require('playwright');
const {runWOSCommand}=require('../extension/wos-adapter.js');

(async()=>{
  const edge='C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
  const browser=await chromium.launch({headless:true,...(fs.existsSync(edge)?{executablePath:edge}:{})});
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'wos-download-integration-'));
  const context=await browser.newContext({acceptDownloads:true});
  const fixture=fs.readFileSync(path.join(__dirname,'fixtures/wos.html'),'utf8');
  await context.route('**/*',route=>route.fulfill({status:200,contentType:'text/html',body:fixture}));
  const page=await context.newPage();
  const origin='https://webofscience.clarivate.cn';
  const listeners=new Set(), downloads=new Map(), saves=[];
  page.on('download',download=>{
    const id=downloads.size+1;
    const item={id,url:download.url(),referrer:page.url(),state:'in_progress',
      filename:path.join(root,`${id}.txt`),fileSize:0};
    downloads.set(id,item);
    for(const listener of listeners)listener({...item});
    saves.push(download.saveAs(item.filename).then(()=>{
      item.fileSize=fs.statSync(item.filename).size;item.state='complete';
    }).catch(error=>{item.state='interrupted';item.error=String(error);}));
  });
  const chrome={tabs:{
    get:async()=>({id:1,url:page.url(),status:'complete'}),
    update:async(id,change)=>{await page.goto(change.url);},
    reload:async()=>{await page.reload();}
  },scripting:{executeScript:async({func,args})=>[{result:await page.evaluate(func,args[0])}]},
  downloads:{onCreated:{addListener:fn=>listeners.add(fn),removeListener:fn=>listeners.delete(fn)},
    search:async({id})=>[downloads.get(id)]}};
  const sandbox=vm.createContext({chrome,URL,Date,setTimeout,runWOSCommand});
  vm.runInContext(fs.readFileSync(path.join(__dirname,'../extension/workflow-background.js'),'utf8'),sandbox);
  const pair={tabId:1,wosTabId:1,mode:'wos'};
  const call=(action,extra={})=>sandbox.dispatchWorkflow({action,sa_id:'download-test',title:'Synthetic paper',
    doi:'10.1234/test',expires:Date.now()+60000,...extra},pair);
  try {
    await page.goto(origin+'/wos/woscc/basic-search');
    // First paper is absent; the next command must clear its stale banner and
    // still search and export, instead of stopping after the expected miss.
    await page.evaluate(()=>main.querySelector('button').onclick=()=>{
      const banner=document.createElement('div');banner.textContent='Your search found no results';main.prepend(banner);
    });
    const missing=await call('wos_search');
    assert.equal(missing.ok,false);assert.match(missing.error,/未找到记录/);
    const found=await call('wos_search');
    assert.equal(found.ok,true,JSON.stringify(found));
    assert.ok(found.data.record_url.endsWith('/WOS:000123456789012'));
    // Guard against a user switching records between search and export.
    const wrong=await call('wos_export',{expected_record_url:found.data.record_url.replace('789012','789013')});
    assert.equal(wrong.ok,false);assert.match(wrong.error,/目标不一致/);
    assert.equal(downloads.size,0);
    // The detail page can arrive before its export controls have mounted.
    await page.evaluate(()=>{
      window.wosOverlay=true;
      const button=document.getElementById('export');button.remove();
      setTimeout(()=>main.append(button),300);
    });
    const exported=await call('wos_export',{expected_record_url:found.data.record_url});
    assert.equal(exported.ok,true,JSON.stringify(exported));
    await Promise.all(saves);
    assert.equal(exported.data.sa_id,'download-test');
    assert.equal(exported.data.record_url,found.data.record_url);
    assert.equal(downloads.size,1);assert.equal(listeners.size,0);
    const txt=fs.readFileSync(exported.data.path,'utf8');
    assert.match(txt,/TI\tAU\tAF\tSO\tPY\tC1\tUT\tDI/);
    assert.match(txt,/Synthetic paper/);assert.match(txt,/WOS:000123456789012/);
    assert.match(txt,/10\.1234\/test/);
    console.log('PASS missing paper -> next search -> exact record -> full-record overlay export -> completed TXT on disk');
    console.log('PASS mismatched record rejected; delayed export controls awaited; download listener cleaned up');
  } finally {
    await context.close();await browser.close();
    await Promise.all(saves);
    // Only this test's freshly created files are removed.
    for(const item of downloads.values())if(fs.existsSync(item.filename))fs.unlinkSync(item.filename);
    fs.rmdirSync(root);
  }
})().catch(error=>{console.error(error);process.exitCode=1;});
