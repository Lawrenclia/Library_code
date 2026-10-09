/* Local-only SA zero -> unique-item fixture. Production never includes this file. */
(function(){
 const ids=new Set(['smoke-4','smoke-pushed','smoke-import']); const states=new Map();let active='';
 const priorQuery=vm.getData.bind(vm);
 vm.getData=async function(){
  const id=this.searchForm.saLzkId;
  if(ids.has(id)){
   if(active===id)states.set(id,copy(synthetic));
   if(!states.has(id))states.set(id,{...copy(synthetic),saLzkId:id,itemId:'',matchCount:0,gh:'001',markStatus:'待处理',remark:'',titleValue:'Synthetic paper',reason:'通讯作者标记不一致；第一作者标记不一致'});
   Object.assign(synthetic,copy(states.get(id)));active=id;
   testConfig.authors=[{id:'zero-author-1',order:1,fullname:'Tester',scholarId:null,institutionOrderNums:[]}];
   testConfig.people=[{id:'scholar-001',wno:'001',nameCn:'测试员',nameEn:'Tester',aliases:[{nameAlias:'Tester'}]}];
   testConfig.saClaim='测试员(001)①';
   testConfig.claim=claimedUsers[1]==='scholar-001'?'已认领':'未认领';
   testConfig.authorInfo='署名：Tester<br/>工号：001<br/>是否第一作者：否<br/>是否通讯作者：否';
   return originalQuery();
  }
  active='';return priorQuery();
 };
 const oldLink=vm.handleEditItem.bind(vm);
 vm.handleEditItem=function(row){
  if(!ids.has(row.saLzkId))return oldLink(row);
  const modal=document.createElement('div');modal.className='el-message-box';modal.innerHTML='<p>请输入匹配条目的平台唯一号</p><input><button>确定</button>';document.body.appendChild(modal);
  modal.querySelector('button').onclick=async()=>{
   const value=modal.querySelector('input').value;
   const response=await fetch('/native/zero-link',{method:'POST',body:JSON.stringify({sa_id:row.saLzkId,item_id:value})});
   if(response.ok){writeCount++;synthetic.itemId=value;synthetic.matchCount=1;modal.remove();await vm.getData();}
  };
 };
 const oldClaim=claimWindow.handleClaim.bind(claimWindow);
 claimWindow.handleClaim=function(author){
  if(!ids.has(synthetic.saLzkId))return oldClaim(author);
  this.loading=true;fetch('/native/zero-claim',{method:'POST'}).then(r=>{if(r.ok)oldClaim(author);});
 };
 const oldComplete=statusModal.handleConfirm.bind(statusModal);
 statusModal.handleConfirm=function(){
  if(!ids.has(this.currentRow?.saLzkId))return oldComplete();
  fetch('/native/zero-complete',{method:'POST'}).then(r=>{if(r.ok)oldComplete();});
 };
})();
