const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const path=require('node:path');
const root=path.join(__dirname,'../extension');

test('WOS pairs and executes without an SA or import tab; rejects backend operations',async()=>{
  const state={}, requests=[], injections=[];
  let listener,command;
  const tab={id:7,status:'complete',url:'https://www.webofscience.com/wos/woscc/basic-search'};
  const chrome={
    runtime:{id:'test',getURL:p=>'chrome-extension://test/'+p,getManifest:()=>({version:'test'}),
      onMessage:{addListener:fn=>listener=fn}},
    storage:{session:{get:async()=>({...state}),clear:async()=>{for(const k of Object.keys(state))delete state[k];},
      set:async values=>Object.assign(state,values)}},
    tabs:{get:async id=>{assert.equal(id,7);return tab;}},
    scripting:{executeScript:async options=>{
      injections.push(options);
      // Pairing is tested through the production split-search dispatcher. A
      // bare data:{} stub no longer implements that read-only state contract.
      if(options.args?.[0]?.action==='wos_start_search'){
        tab.url='https://www.webofscience.com/wos/woscc/full-record/WOS:000123456789012';
        return [{result:{ok:true,data:{submitted:true}}}];
      }
      return [{result:{ok:true,data:{state:'record',record_url:tab.url}}}];
    }}
  };
  const context=vm.createContext({chrome,URL,Date,AbortSignal,importScripts:()=>{},
    runSACommand:()=>{throw Error('must not call SA');},
    fetch:async(url,opts)=>{
      requests.push({url,body:JSON.parse(opts.body)});
      if(url.endsWith('/ack'))return {ok:true,json:async()=>({accepted:true})};
      return {ok:true,json:async()=>({command:JSON.parse(opts.body).claimOnly?null:command})};
    },runWOSCommand:()=>{},inspectWorkPage:()=>{},setTimeout});
  vm.runInContext(fs.readFileSync(path.join(root,'workflow-background.js'),'utf8'),context);
  vm.runInContext(fs.readFileSync(path.join(root,'background.js'),'utf8'),context);
  const send=(message,sender)=>new Promise(resolve=>listener(message,sender,resolve));
  const paired=await send({type:'pair',tabId:7,token:'a'.repeat(43)},
    {id:'test',url:'chrome-extension://test/popup.html'});
  assert.equal(paired.ok,true);
  assert.equal(state.mode,'wos');
  assert.equal(state.wosTabId,7);
  assert.equal(state.importTabId,undefined);
  command={id:'one',action:'wos_search',expires:Date.now()+60000};
  await send({type:'tick'},{tab});
  assert.equal(requests.at(-1).body.result.ok,true,JSON.stringify(requests.at(-1).body.result));
  assert.equal(injections.at(-1).target.tabId,7);
  const count=injections.length;
  command={id:'two',action:'import_submit',expires:Date.now()+60000};
  await send({type:'tick'},{tab});
  assert.equal(requests.at(-1).body.result.ok,false);
  assert.equal(injections.length,count);
});
