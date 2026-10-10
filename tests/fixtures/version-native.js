/* Local-only input-version fixture; no platform writes are performed here. */
(function(){
 const priorVersionQuery=vm.getData.bind(vm);
 vm.getData=async function(){
  if(this.searchForm.saLzkId!=='smoke-version')return priorVersionQuery();
  Object.assign(synthetic,{saLzkId:'smoke-version',gh:'001',titleValue:'Synthetic paper',doiValue:'10.1234/test',wosValue:'',matchCount:0,itemId:'',reason:'测试',markStatus:'待处理',remark:''});
  return originalQuery();
 };
})();
