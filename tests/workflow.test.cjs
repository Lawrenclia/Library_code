const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const {chromium}=require('playwright');
const {runImportCommand}=require('../extension/import-adapter.js');
const {runWOSCommand}=require('../extension/wos-adapter.js');
const {inspectWorkPage}=require('../extension/page-diagnostics.js');
const fixture=fs.readFileSync(path.join(__dirname,'fixtures/import.html'),'utf8');
const wosFixture=fs.readFileSync(path.join(__dirname,'fixtures/wos.html'),'utf8');
const edge='C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const candidate={title:'Synthetic paper',doi:'10.1234/test',wos:'WOS:000123456789012',sjtu:true,sha256:'a'.repeat(64)};
const cmd=(action,more={})=>({action,sa_id:'demo-001',instructions:'SA补充-demo-001',candidate,expires:Date.now()+30000,...more});
(async()=>{
  const browser=await chromium.launch({headless:true,...(fs.existsSync(edge)?{executablePath:edge}:{})});
  const context=await browser.newContext();
  await context.route('**/*',r=>r.fulfill({status:200,contentType:'text/html',body:new URL(r.request().url()).hostname==='admin.ir.lib.sjtu.edu.cn'?fixture:wosFixture}));
  const page=await context.newPage();
  const reset=async()=>{await page.goto('http://admin.ir.lib.sjtu.edu.cn/#/collectItem/batchManage');await page.reload();};
  const execute=c=>page.evaluate(runImportCommand,c);
  const upload=async()=>{
    const r=await execute(cmd('import_upload',{content:Buffer.from('synthetic TXT').toString('base64'),contentSha:candidate.sha256}));
    assert.equal(r.ok,true,JSON.stringify(r));return r.data;
  };
  const submit=async()=>{const data=await upload();const r=await execute(cmd('import_submit',{upload:data}));assert.equal(r.ok,true,JSON.stringify(r));};
  const tests=[];const test=(name,fn)=>tests.push([name,fn]);
  test('empty scan performs no writes',async()=>{
    assert.deepEqual((await execute(cmd('import_scan'))).data.batches,[]);
    assert.deepEqual(await page.evaluate(()=>writes),{upload:0,import:0,push:0});
  });
  test('complete import and priority-merge push verify original title and identifiers',async()=>{
    await submit();
    const checked=await execute(cmd('import_check'));
    assert.equal(checked.ok,true,JSON.stringify(checked));
    assert.equal(checked.data.batch.actual,0);
    assert.equal(checked.data.items[0].metadata.wosId[0],candidate.wos,'回读保留完整文献字段');
    const pushed=await execute(cmd('import_push',{batch:checked.data.batch}));assert.equal(pushed.ok,true,JSON.stringify(pushed));
    const verified=await execute(cmd('import_check',{batch_id:'batch-001',expect_pushed:true}));
    assert.equal(verified.data.batch.status,2);
    assert.equal(await page.evaluate(()=>push.form.duplicateItemProcessingType),'4');
    assert.equal(await page.evaluate(()=>push.form.duplicateQueryType),'ppt-composite');
    assert.equal(pushed.data.push_settings.duplicateItemProcessingType,'4');
    assert.match(pushed.data.duplicate_query_label,/题名相似度.*题名相似度/);
    assert.deepEqual(await page.evaluate(()=>writes),{upload:1,import:1,push:1});
  });
  test('wrong route and foreign drawer never mutate',async()=>{
    await page.evaluate(()=>location.hash='#/wel/index');assert.equal((await execute(cmd('import_scan'))).ok,false);
    await reset();await page.evaluate(()=>drawer.drawer=true);
    assert.match((await execute(cmd('import_upload'))).error,/人工打开/);
    assert.equal(await page.evaluate(()=>writes.upload),0);
  });
  test('double upload and double import are refused',async()=>{
    const u=await upload();assert.equal((await execute(cmd('import_upload'))).ok,false);
    assert.equal((await execute(cmd('import_submit',{upload:u}))).ok,true);
    assert.equal((await execute(cmd('import_submit',{upload:u}))).ok,false);
    assert.equal(await page.evaluate(()=>writes.import),1);
  });
  test('lost upload confirmation restores only the original upload with no repeated write',async()=>{
    await upload();await page.evaluate(()=>{drawer.__saImport.uploaded=false;delete drawer.__saImport.serverName;});
    const read=await execute(cmd('import_check',{expect_upload:true,content:Buffer.from('synthetic TXT').toString('base64'),contentSha:candidate.sha256}));
    assert.equal(read.ok,true,JSON.stringify(read));assert.equal(read.data.verified,true);assert.equal(read.data.uploaded,true);
    assert.equal(read.data.instructions,'SA补充-demo-001');assert.equal(read.data.server_name,'synthetic-object.txt');
    assert.deepEqual(await page.evaluate(()=>writes),{upload:1,import:0,push:0});
    assert.equal((await execute(cmd('import_submit',{upload:read.data}))).ok,true);
    assert.equal(await page.evaluate(()=>writes.import),1);
  });
  test('upload readback refuses changed original bytes, form, response and checkpoint',async()=>{
    for(const scenario of ['bytes','description','institution','response','logicalFailure','missingMarker','markerChange','fileSwap','formDuringRead']){
      await reset();await upload();
      await page.evaluate(s=>{const input=drawer.$el.querySelector('input');
        if(s==='bytes'){const dt=new DataTransfer();dt.items.add(new File(['wrong content'],input.files[0].name));input.files=dt.files;}
        if(s==='description')drawer.form.instructions='SA补充-other';
        if(s==='institution')drawer.form.datasetId='other';
        if(s==='response')drawer.fileList[0].response={success:false};
        if(s==='logicalFailure')drawer.fileList[0].response={success:false,code:200,data:{name:'synthetic-object.txt'}};
        if(s==='missingMarker')delete drawer.__saImport;
        if(['markerChange','fileSwap','formDuringRead'].includes(s)){
          const file=input.files[0],original=file.arrayBuffer.bind(file);
          file.arrayBuffer=async()=>{const bytes=await original();
            if(s==='markerChange')drawer.__saImport.sha256='b'.repeat(64);
            if(s==='formDuringRead')drawer.form.instructions='SA补充-other';
            if(s==='fileSwap'){const dt=new DataTransfer();dt.items.add(new File([bytes],file.name));input.files=dt.files;}
            return bytes;};
        }
      },scenario);
      const read=await execute(cmd('import_check',{expect_upload:true,content:Buffer.from('synthetic TXT').toString('base64'),contentSha:candidate.sha256}));
      assert.equal(read.ok,false,scenario);assert.deepEqual(await page.evaluate(()=>writes),{upload:1,import:0,push:0},scenario);
    }
  });
  test('closed upload without a batch stays unknown; an actual verified batch can recover',async()=>{
    const data=await upload();await page.evaluate(()=>drawer.drawer=false);
    let read=await execute(cmd('import_check',{expect_upload:true}));assert.equal(read.ok,false);assert.equal(read.code,'REMOTE_RESULT_UNKNOWN');
    assert.deepEqual(await page.evaluate(()=>writes),{upload:1,import:0,push:0});
    await page.evaluate(()=>drawer.drawer=true);await execute(cmd('import_submit',{upload:data}));
    read=await execute(cmd('import_check',{expect_upload:true}));assert.equal(read.ok,true,JSON.stringify(read));assert.equal(read.data.batch.status,1);
    assert.deepEqual(await page.evaluate(()=>writes),{upload:1,import:1,push:0});
  });
  test('HTTP 200 with explicit upload failure cannot become an uploaded checkpoint',async()=>{
    await page.evaluate(()=>{const input=drawer.$el.querySelector('input');input.addEventListener('change',()=>setTimeout(()=>{
      drawer.fileList[0].response={success:false,code:200,data:{name:'not-confirmed.txt'}};
    },35));});
    const result=await execute(cmd('import_upload',{content:Buffer.from('synthetic TXT').toString('base64'),contentSha:candidate.sha256}));
    assert.equal(result.ok,false);assert.equal(result.submitted,true);assert.equal(await page.evaluate(()=>drawer.__saImport.uploaded),false);
    assert.deepEqual(await page.evaluate(()=>writes),{upload:1,import:0,push:0});
  });
  test('wrong instructions and unhashed upload are refused',async()=>{
    assert.equal((await execute(cmd('import_upload',{instructions:'SA补充-other'}))).ok,false);
    assert.equal((await execute(cmd('import_upload',{content:'eA==',contentSha:'b'.repeat(64)}))).ok,false);
    assert.equal(await page.evaluate(()=>writes.upload),0);
  });
  test('changed institution or description after upload stops submission',async()=>{
    const u=await upload();await page.evaluate(()=>drawer.form.datasetId='other');
    assert.equal((await execute(cmd('import_submit',{upload:u}))).ok,false);
    assert.equal(await page.evaluate(()=>writes.import),0);
  });
  test('pre-existing same-description batch never uploads again',async()=>{
    await submit();await page.evaluate(()=>delete drawer.__saImport);
    assert.equal((await execute(cmd('import_upload'))).ok,false);assert.equal(await page.evaluate(()=>writes.upload),1);
  });
  test('wrong imported identifiers prevent push',async()=>{
    await submit();await page.evaluate(()=>testConfig.wrongUT=true);
    assert.match((await execute(cmd('import_check'))).error,/入藏号/);
    assert.equal(await page.evaluate(()=>writes.push),0);
  });
  test('duplicate instructions cannot select first batch',async()=>{
    await submit();await execute(cmd('import_scan'));
    await page.evaluate(()=>batches.push({...batches[0],id:'batch-002'}));
    assert.match((await execute(cmd('import_check'))).error,/唯一/);
  });
  test('missing PPT dedup option stops, never falls back to defaults',async()=>{
    await submit();const r=await execute(cmd('import_check'));
    await page.evaluate(()=>push.duplicateQueryTypes=[{value:'default',label:'唯一标识'}]);
    assert.match((await execute(cmd('import_push',{batch:r.data.batch}))).error,/PPT/);
    assert.equal(await page.evaluate(()=>writes.push),0);
  });
  test('PPT merge and new-item enums come from visible labels, not fixed numbers',async()=>{
    await submit();const checked=await execute(cmd('import_check'));
    await page.evaluate(()=>{push.$children.find(v=>v.prop==='duplicateItemProcessingType').$children[0].label='live-priority';push.$children.find(v=>v.prop==='newItemProcessingType').$children[0].label='live-create';});
    const pushed=await execute(cmd('import_push',{batch:checked.data.batch}));assert.equal(pushed.ok,true,JSON.stringify(pushed));
    assert.equal(await page.evaluate(()=>push.form.duplicateItemProcessingType),'live-priority');
    assert.equal(await page.evaluate(()=>push.form.newItemProcessingType),'live-create');
  });
  test('missing, duplicate, changed or disabled PPT radio choices stop before push',async()=>{
    for(const scenario of ['missing','duplicate','changed','disabled','hidden','boolean']){
      await reset();await submit();const checked=await execute(cmd('import_check'));
      await page.evaluate(s=>{const item=push.$children.find(v=>v.prop==='duplicateItemProcessingType');const radio=item.$children[0];
        if(s==='missing')item.$children=[];
        if(s==='duplicate')item.$children.push({...radio});
        if(s==='changed')radio.$el.textContent='整体覆盖';
        if(s==='disabled')radio.isDisabled=true;
        if(s==='hidden')radio.$el.style.display='none';
        if(s==='boolean')push.$children.find(v=>v.prop==='owner').$children[0].label=false;
      },scenario);
      const result=await execute(cmd('import_push',{batch:checked.data.batch}));assert.equal(result.ok,false,scenario);assert.equal(result.submitted,false,scenario);
      assert.equal(await page.evaluate(()=>writes.push),0,scenario);
    }
  });
  test('WOS author search and foreign hosts are rejected',async()=>{
    await page.goto('https://www.webofscience.com/wos/author/author-search');
    assert.equal((await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'})).ok,false);
    for(const origin of ['http://webofscience.clarivate.cn','https://webofscience.clarivate.cn.example.invalid','https://www.webofscience.com.example.invalid']) {
      await page.goto(origin+'/wos/woscc/basic-search');
      assert.equal((await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'})).ok,false);
      assert.equal(await page.evaluate(()=>searches),0);
      assert.equal((await page.evaluate(inspectWorkPage)).error,'非工作网站');
    }
  });
  test('expired commands are rejected before scanning',async()=>{
    assert.equal((await execute(cmd('import_scan',{expires:Date.now()-1}))).ok,false);
  });
  test('WOS title query opens the single record and selects Full Record export',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    const base={...cmd('wos_search'),title:'Synthetic paper',doi:'10.1234/test',wos:''};
    const found=await page.evaluate(runWOSCommand,base);assert.equal(found.ok,true,JSON.stringify(found));
    const prep=await page.evaluate(runWOSCommand,{...base,action:'wos_prepare_export'});
    assert.equal(prep.ok,true,JSON.stringify(prep));
    assert.equal(await page.evaluate(()=>document.querySelector('select').value),'Full Record');
    const download=page.waitForEvent('download');
    const done=await page.evaluate(runWOSCommand,{...base,action:'wos_download'});
    assert.equal(done.ok,true,JSON.stringify(done));await download;
    assert.equal((await page.evaluate(runWOSCommand,{...base,action:'wos_download'})).ok,false);
    assert.equal(await page.evaluate(()=>exportsMade),1);
  });
  test('WOS split search returns before navigation and exposes one safe read-only result',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    const base={...cmd('wos_start_search'),title:'Synthetic paper',doi:'',wos:''};
    const started=await page.evaluate(runWOSCommand,base);
    assert.equal(started.ok,true,JSON.stringify(started));assert.equal(started.data.submitted,true);
    await page.waitForFunction(()=>searches===1);
    const read=await page.evaluate(runWOSCommand,{...base,action:'wos_read_results'});
    assert.equal(read.ok,true,JSON.stringify(read));assert.equal(read.data.state,'single');
    assert.ok(page.url().includes('/summary/'),'read-only probe must not click the result');
    assert.match(read.data.navigate_url,/\/full-record\/WOS:000123456789012$/);
    await page.goto(read.data.navigate_url);
    const verified=await page.evaluate(runWOSCommand,{...base,action:'wos_read_results'});
    assert.equal(verified.data.state,'record');assert.equal(verified.data.record_url,read.data.navigate_url);
  });
  test('101 results with one exact title open the matching detail and download through overlay',async()=>{
    await page.goto('https://webofscience.clarivate.cn/wos/woscc/basic-search');
    await page.evaluate(()=>{
      window.wosOverlay=true;
      const search=document.querySelector('button'), original=search.onclick;
      search.onclick=()=>{
        original();
        history.replaceState({},'', '/wos/woscc/summary/search-id/session-id');
        document.querySelector('h1').textContent='101 results from Web of Science Core Collection';
        main.insertAdjacentHTML('afterbegin','<a href="/wos/woscc/full-record/WOS:000123456789099">Unrelated paper</a><button>Export</button>');
      };
    });
    const base={...cmd('wos_search'),title:'SYNTHETIC PAPER'};
    const found=await page.evaluate(runWOSCommand,base);
    assert.equal(found.ok,true,JSON.stringify(found));
    assert.ok(page.url().endsWith('/full-record/WOS:000123456789012'));
    const prepared=await page.evaluate(runWOSCommand,{...base,action:'wos_prepare_export'});
    assert.equal(prepared.ok,true,JSON.stringify(prepared));
    assert.ok(page.url().endsWith('(overlay:export/ext)'));
    assert.equal(prepared.data.record_url,found.data.record_url);
    const download=page.waitForEvent('download');
    const done=await page.evaluate(runWOSCommand,{...base,action:'wos_download'});
    assert.equal(done.ok,true,JSON.stringify(done));
    assert.equal((await download).suggestedFilename(),'wos-synthetic.txt');
    assert.equal(await page.evaluate(()=>exportsMade),1);
  });
  test('overlay on a different record cannot submit the prepared export',async()=>{
    await page.goto('https://webofscience.clarivate.cn/wos/woscc/full-record/WOS:000123456789012');
    const base={...cmd('wos_prepare_export'),title:'Synthetic paper'};
    assert.equal((await page.evaluate(runWOSCommand,base)).ok,true);
    await page.evaluate(()=>history.replaceState({},'', '/wos/woscc/full-record/WOS:000123456789013(overlay:export/ext)'));
    assert.equal((await page.evaluate(runWOSCommand,{...base,action:'wos_download'})).ok,false);
    assert.equal(await page.evaluate(()=>exportsMade),0);
  });
  test('same-URL search accepts newly rendered results',async()=>{
    await page.goto('https://webofscience.clarivate.cn/wos/woscc/basic-search');
    await page.evaluate(()=>{
      const button=main.querySelector('button'), original=button.onclick;
      button.onclick=()=>{const previous=location.href;original();history.replaceState({},'',previous);};
    });
    const result=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'});
    assert.equal(result.ok,true,JSON.stringify(result));
    assert.ok(page.url().includes('/full-record/'));
  });
  test('same-URL no-results message is reported without waiting for navigation',async()=>{
    await page.goto('https://webofscience.clarivate.cn/wos/woscc/basic-search');
    await page.evaluate(()=>main.querySelector('button').onclick=()=>{main.innerHTML='<div role="alert"><strong>Your search found no results</strong><p>Check the spelling and/or broaden your search parameters</p></div>';});
    const result=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'});
    assert.equal(result.ok,false);assert.match(result.error,/未找到记录/);
  });
  test('successive searches recognize a replaced no-results banner on the same form',async()=>{
    await page.goto('https://webofscience.clarivate.cn/wos/woscc/basic-search');
    await page.evaluate(()=>{
      const banner=document.createElement('div');banner.textContent='Your search found no results';main.append(banner);
      main.querySelector('button').onclick=()=>{searches++;banner.remove();main.append(banner);};
    });
    for(const title of ['First missing paper','Second missing paper']){
      const result=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title});
      assert.equal(result.ok,false);assert.match(result.error,/未找到记录/);
    }
    assert.equal(await page.evaluate(()=>searches),2);
  });
  test('unchanged previous no-results banner does not decide the next query',async()=>{
    await page.goto('https://webofscience.clarivate.cn/wos/woscc/basic-search');
    await page.evaluate(()=>{
      main.insertAdjacentHTML('beforeend','<strong>Your search found no results</strong>');
      main.querySelector('button').onclick=()=>{};
    });
    const result=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Next paper',expires:Date.now()+13300});
    assert.equal(result.ok,false);assert.match(result.error,/新结果/);
  });
  test('changed URL with unchanged stale records cannot export old results',async()=>{
    await page.goto('https://webofscience.clarivate.cn/wos/woscc/basic-search');
    await page.evaluate(()=>{
      main.insertAdjacentHTML('beforeend','<h1>1 result</h1><a href="/wos/woscc/full-record/WOS:000123456789012">Synthetic paper</a>');
      main.querySelector('button').onclick=()=>history.pushState({},'', '/wos/woscc/summary/new-query');
    });
    const result=await page.evaluate(runWOSCommand,{...cmd('wos_search'),expires:Date.now()+13300,title:'Synthetic paper'});
    assert.equal(result.ok,false);assert.match(result.error,/新结果/);
    assert.equal(await page.evaluate(()=>exportsMade),0);
    assert.ok(page.url().includes('/summary/'));
  });
  test('WOS multiple matches pause without opening the first record',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    await page.evaluate(()=>wosMany=true);
    const r=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'});
    assert.equal(r.ok,false);assert.match(r.error,/唯一/);assert.ok(page.url().includes('/summary/'));
  });
  test('WOS accepts one delayed same-origin record link with an encoded accession colon',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    await page.evaluate(()=>{
      document.querySelector('button').onclick=()=>{
        searches++;history.pushState({},'', '/wos/woscc/summary/encoded');
        document.getElementById('main').innerHTML='<p>Loading records</p>';
        setTimeout(()=>{
          document.getElementById('main').innerHTML='<h1>1 result</h1><a href="/wos/woscc/full-record/WOS%3A000123456789012">Synthetic paper</a>';
          document.querySelector('a').onclick=event=>{event.preventDefault();history.pushState({},'',event.currentTarget.getAttribute('href'));};
        },350);
      };
    });
    const r=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'});
    assert.equal(r.ok,true,JSON.stringify(r));assert.match(r.data.record_url,/WOS%3A000123456789012/i);
    assert.equal(await page.evaluate(()=>searches),1);
  });
  test('WOS encoded multiple records remain ambiguous and none is opened',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    await page.evaluate(()=>{
      window.openedEncoded=0;
      document.querySelector('button').onclick=()=>{
        searches++;history.pushState({},'', '/wos/woscc/summary/encoded-many');
        document.getElementById('main').innerHTML='<h1>2 results</h1><a href="/wos/woscc/full-record/WOS%3A000123456789012">One</a><a href="/wos/woscc/full-record/WOS%3A000123456789013">Two</a>';
        document.querySelectorAll('a').forEach(anchor=>anchor.onclick=event=>{event.preventDefault();openedEncoded++;});
      };
    });
    const r=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'});
    assert.equal(r.ok,false);assert.match(r.error,/唯一/);assert.equal(await page.evaluate(()=>openedEncoded),0);
  });
  test('WOS Chinese and English zero-result banners finish without URL navigation or timeout',async()=>{
    const cases=[['First missing paper','您的检索未找到结果'],
      ['Second missing paper','Your search did not return any results'],
      ['Third missing paper','Your search did not find any results']];
    for(const [title,message] of cases){
      await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
      await page.evaluate(message=>document.querySelector('button').onclick=()=>{
        searches++;const alert=document.createElement('section');alert.setAttribute('role','alert');
        alert.textContent=message;document.getElementById('main').prepend(alert);
      },message);
      const r=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title});
      assert.equal(r.ok,false);assert.match(r.error,/WOS 未找到记录/);
      assert.ok(page.url().endsWith('/wos/woscc/basic-search'));
      assert.equal(await page.evaluate(()=>searches),1);
    }
  });
  test('WOS read-only result probe recognizes a Chinese zero banner split across DOM nodes',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    const base={...cmd('wos_start_search'),title:'Missing paper'};
    await page.evaluate(()=>document.querySelector('button').onclick=()=>{
      searches++;const alert=document.createElement('section');alert.setAttribute('role','alert');
      alert.innerHTML='<span>您的</span><span>检索</span><span>未找到</span><span>结果</span>';
      document.getElementById('main').prepend(alert);
    });
    const started=await page.evaluate(runWOSCommand,base);assert.equal(started.ok,true,JSON.stringify(started));
    await page.waitForFunction(()=>searches===1);
    const read=await page.evaluate(runWOSCommand,{...base,action:'wos_read_results'});
    assert.equal(read.ok,true,JSON.stringify(read));assert.equal(read.data.state,'zero');
  });
  test('WOS stale zero-result banner requests a clean reload before any new search click',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    await page.evaluate(()=>{const alert=document.createElement('section');alert.setAttribute('role','alert');
      alert.textContent='您的检索未找到结果';document.getElementById('main').prepend(alert);});
    const r=await page.evaluate(runWOSCommand,{...cmd('wos_start_search'),title:'Next paper'});
    assert.equal(r.ok,false);assert.match(r.error,/保留上一条零结果/);
    assert.equal(await page.evaluate(()=>searches),0);
  });
  test('read-only diagnosis reports encoded WOS links without exposing titles or queries',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    await page.evaluate(()=>{history.pushState({},'', '/wos/woscc/summary/diagnostic');
      document.getElementById('main').innerHTML='<h1>1 result</h1><a href="/wos/woscc/full-record/WOS%3A000123456789012">DO_NOT_DISCLOSE_TITLE</a>';});
    const d=await page.evaluate(inspectWorkPage);
    assert.equal(d.summary_route,true);assert.equal(d.canonical_record_link_count,1);assert.equal(d.encoded_record_link_count,1);
    assert.ok(!JSON.stringify(d).includes('DO_NOT_DISCLOSE_TITLE'));
  });
  test('PPT Chinese results list selects one paper and downloads Full Record TXT',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    await page.evaluate(()=>{
      document.querySelector('button').onclick=()=>{
        searches++;
        recordPage();
        const exportAction=document.getElementById('export').onclick;
        history.pushState({},'', '/wos/woscc/summary/ppt-example/session-id');
        const main=document.getElementById('main');
        main.innerHTML='<h1>1 文献</h1><article><input type="checkbox"><a href="/wos/woscc/full-record/WOS:000123456789012">Synthetic paper</a></article><button id="export" disabled><span>导出</span><mat-icon>expand_more</mat-icon></button>';
        const button=document.getElementById('export');
        document.querySelector('input').onchange=e=>button.disabled=!e.target.checked;
        button.onclick=()=>{
          exportAction();
          const menu=main.lastElementChild;
          menu.innerHTML='<span>制表符分隔文件</span><mat-icon>download</mat-icon>';
        };
      };
    });
    const base={...cmd('wos_search'),title:'Synthetic paper',doi:'10.1234/test'};
    const found=await page.evaluate(runWOSCommand,base);
    assert.equal(found.ok,true,JSON.stringify(found));
    assert.ok(page.url().includes('/summary/'));
    const prep=await page.evaluate(runWOSCommand,{...base,action:'wos_prepare_export'});
    assert.equal(prep.ok,true,JSON.stringify(prep));
    assert.equal(await page.locator('input[type="checkbox"]').isChecked(),true);
    const download=page.waitForEvent('download');
    const result=await page.evaluate(runWOSCommand,{...base,action:'wos_download'});
    assert.equal(result.ok,true,JSON.stringify(result));
    assert.equal((await download).suggestedFilename(),'wos-synthetic.txt');
    assert.equal(await page.evaluate(()=>exportsMade),1);
  });
  test('WOS Oops page reports site failure before field selection or any search',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    await page.evaluate(()=>document.getElementById('main').innerHTML="<h1>Oops, something went wrong!</h1><p>Please click on 'Search' at the top of the screen.</p>");
    const r=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'});
    assert.match(r.error,/WOS 网站自身报错/);assert.doesNotMatch(r.error,/选择器未唯一/);
    assert.equal(await page.evaluate(()=>searches),0);
  });
  test('WOS Smart Search follows visible Advanced and Fielded tabs, without changing preferences',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    await page.evaluate(()=>smartPage());
    const r=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'});
    assert.equal(r.ok,true,JSON.stringify(r));assert.equal(await page.evaluate(()=>searches),1);
  });
  test('WOS waits for the SPA to mount a selected Fielded Search row',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    await page.evaluate(()=>{
      const main=document.getElementById('main');
      main.innerHTML='<button role="tab" aria-selected="true">字段检索</button><p>正在加载</p>';
      setTimeout(()=>searchPage(),350);
    });
    const r=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'});
    assert.equal(r.ok,true,JSON.stringify(r));assert.equal(await page.evaluate(()=>searches),1);
  });
  test('fielded navigation outranks persistent Advanced links and waits for the selected form',async()=>{
    await page.goto('https://webofscience.clarivate.cn/wos/woscc/basic-search');
    await page.evaluate(()=>{
      main.innerHTML='<nav><a href="/wos/woscc/advanced-search">Advanced Search</a><a href="/wos/woscc/advanced-search">Advanced Search</a></nav><div role="button">Fielded Search</div>';
      main.querySelector('[role="button"]').onclick=e=>{
        e.currentTarget.setAttribute('aria-selected','true');
        setTimeout(()=>searchPage(),350);
      };
    });
    const result=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'});
    assert.equal(result.ok,true,JSON.stringify(result));
    assert.equal(await page.evaluate(()=>searches),1);
  });
  test('duplicate Advanced anchors to the same route are one entrance',async()=>{
    await page.goto('https://webofscience.clarivate.cn/wos/');
    await page.evaluate(()=>{
      main.innerHTML='<nav><a href="/wos/woscc/advanced-search">Advanced Search</a><a href="/wos/woscc/advanced-search">Advanced Search</a></nav>';
      for(const a of main.querySelectorAll('a'))a.onclick=e=>{
        e.preventDefault();history.pushState({},'',a.href);
        main.innerHTML='<button role="tab">Fielded Search</button>';
        main.querySelector('button').onclick=()=>searchPage();
      };
    });
    const result=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'});
    assert.equal(result.ok,true,JSON.stringify(result));
  });
  test('different Advanced destinations remain ambiguous and diagnosis includes anchors',async()=>{
    await page.goto('https://webofscience.clarivate.cn/wos/');
    await page.evaluate(()=>main.innerHTML='<a href="/wos/woscc/advanced-search">Advanced Search</a><a href="/wos/author/advanced-search">Advanced Search</a>');
    const result=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'});
    assert.equal(result.ok,false);assert.match(result.error,/入口不唯一/);
    const diagnostic=await page.evaluate(inspectWorkPage);
    assert.equal(diagnostic.navigation.length,2);
    assert.equal(diagnostic.navigation[0].label,'Advanced Search');
  });
  test('WOS multiple field rows are still refused',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    await page.evaluate(()=>document.getElementById('main').appendChild(document.querySelector('select').cloneNode(true)));
    const r=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'});
    assert.match(r.error,/识别到 2 个/);assert.equal(await page.evaluate(()=>searches),0);
  });
  test('CN landing page waits for delayed Advanced Search then enters fielded search',async()=>{
    await page.goto('https://webofscience.clarivate.cn/wos/');
    await page.evaluate(()=>{
      document.getElementById('main').innerHTML='<h1>Smart Search</h1>';
      setTimeout(()=>smartPage(),250);
    });
    const r=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'});
    assert.equal(r.ok,true,JSON.stringify(r));
    assert.equal(await page.evaluate(()=>searches),1);
    assert.ok(page.url().includes('/wos/woscc/full-record/'));
  });
  test('WOS Chinese all-fields selector is supported',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    await page.evaluate(()=>document.querySelector('select').options[0].textContent='所有字段');
    const r=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'});
    assert.equal(r.ok,true,JSON.stringify(r));
  });
  test('WOS Oops arising during search is not retried',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    await page.evaluate(()=>document.querySelector('button').onclick=()=>{searches++;document.getElementById('main').innerHTML='Oops, something went wrong!';});
    const r=await page.evaluate(runWOSCommand,{...cmd('wos_search'),title:'Synthetic paper'});
    assert.match(r.error,/WOS 网站自身报错/);assert.equal(await page.evaluate(()=>searches),1);
  });
  test('read-only diagnosis detects error page and excludes entered input text',async()=>{
    await page.goto('https://www.webofscience.com/wos/woscc/basic-search');
    await page.evaluate(()=>{document.querySelector('input').value='DO_NOT_DISCLOSE_QUERY';document.querySelector('input').setAttribute('role','combobox');
      const p=document.createElement('p');p.textContent='Oops, something went wrong!';document.body.appendChild(p);});
    const d=await page.evaluate(inspectWorkPage);assert.equal(d.wos_error,true);
    assert.ok(!JSON.stringify(d).includes('DO_NOT_DISCLOSE_QUERY'));assert.equal(await page.evaluate(()=>searches),0);
  });
  test('Clarivate CN supports diagnosis, title search, and one Full Record export on the same origin',async()=>{
    const origin='https://webofscience.clarivate.cn';
    await page.goto(origin+'/wos/woscc/basic-search');
    const diagnostic=await page.evaluate(inspectWorkPage);
    assert.equal(diagnostic.site,'webofscience.clarivate.cn');assert.equal(diagnostic.controls.length,1);
    const base={...cmd('wos_search'),title:'Synthetic paper'};
    const found=await page.evaluate(runWOSCommand,base);assert.equal(found.ok,true,JSON.stringify(found));
    assert.ok(found.data.record_url.startsWith(origin+'/wos/woscc/full-record/'));
    const prepared=await page.evaluate(runWOSCommand,{...base,action:'wos_prepare_export'});
    assert.equal(prepared.ok,true,JSON.stringify(prepared));assert.equal(await page.evaluate(()=>document.querySelector('select').value),'Full Record');
    const downloaded=page.waitForEvent('download');
    assert.equal((await page.evaluate(runWOSCommand,{...base,action:'wos_download'})).ok,true);
    await downloaded;assert.equal(await page.evaluate(()=>exportsMade),1);
    assert.equal((await page.evaluate(runWOSCommand,{...base,action:'wos_download'})).ok,false);
  });
  try{for(const [name,fn] of tests){await reset();await fn();console.log('PASS '+name);}console.log(`Workflow adapters: ${tests.length} offline cases passed.`);}
  finally{await context.close();await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
