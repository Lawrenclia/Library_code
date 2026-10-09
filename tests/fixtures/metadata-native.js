// Synthetic editor layered onto compare.html for the actual Tauri smoke run.
(() => {
  const host=document.createElement('div');host.id='native-edit-owner';host.style.display='none';host.innerHTML='<div id="native-edit" class="el-drawer"></div>';document.body.appendChild(host);
  let saved={id:'item-metadata',modelId:'model-fixture',modelName:'期刊论文',datasetIds:['dataset-fixture'],dataSources:['WOS'],fullTexts:[],metadata:{title:['Synthetic paper'],doi:['10.1234/test'],pages:['10-20'],abstract:['完整原文摘要仍保留'],author:[
    {id:'native-author-1',fullname:'Other',order:1,scholarId:'',institutionOrderNums:'1',correspondent:false,commonFirst:false},
    {id:'native-author-2',fullname:'Tester',order:2,scholarId:'scholar-001',institutionOrderNums:'1',correspondent:false,commonFirst:false}],authorInstitution:[{order:1,address:'Shanghai Jiao Tong University',topInstitutionId:'1244586319225556993'}]}};
  const fields=()=>[{fieldName:'author',reveal:1,dataType:4,editable:1,customConfig:{correspondent:true,commonFirst:true}}];
  const editor={$options:{name:'itemModelEdit'},$el:document.getElementById('native-edit'),itemId:'item-metadata',loading:false,auth:false,ruleForm:copy(saved),modelFieldList:fields(),validInfo:{author:{}},
    $nextTick:async()=>render(),
    async getItemDetailData(){this.loading=true;this.ruleForm={};await new Promise(resolve=>setTimeout(resolve,5));this.ruleForm=copy(saved);
      for(const author of this.ruleForm.metadata.author)if(Array.isArray(author.institutionOrderNums))author.institutionOrderNums=author.institutionOrderNums.toString();
      this.modelFieldList=fields();this.loading=false;render();},
    onSubmit(){this.submitData(this.ruleForm);},
    async submitData(form){this.loading=true;for(const author of form.metadata.author)author.institutionOrderNums=author.institutionOrderNums.split(',');
      const response=await fetch('/native/metadata-save',{method:'POST'});if(response.ok){saved=copy(form);this.getItemDetailData();}this.loading=false;}
  };
  const owner={drawer:false,shows:false,itemId:'',$el:host,$refs:{itemModelEdit:editor},handleCloses(){this.drawer=false;this.shows=false;host.style.display='none';}};
  detail.$refs.itemEdit=owner;
  detail.editItem=function(id){owner.itemId=id;owner.drawer=true;owner.shows=true;editor.itemId=id;host.style.display='block';editor.getItemDetailData();};
  host.__vue__=editor;
  function render(){editor.$el.innerHTML='';for(const [index,author]of (editor.ruleForm.metadata?.author||[]).entries()){
    const line=document.createElement('div');line.className='el-form-item';const name=document.createElement('div');name.innerHTML='<input>';name.querySelector('input').value=author.fullname;
    name.__vue__={$options:{name:'ElInput'},$el:name,value:author.fullname,$vnode:{data:{model:{expression:'val.fullname'}}}};line.appendChild(name);
    for(const field of ['correspondent','commonFirst'])if(!(index===0&&field==='commonFirst')){const checkbox=document.createElement('label');checkbox.innerHTML='<input type="checkbox">'+field;const input=checkbox.querySelector('input');input.checked=author[field];
      checkbox.__vue__={$options:{name:'ElCheckbox'},$el:checkbox,value:author[field],$vnode:{data:{model:{expression:'val.'+field,callback:value=>{author[field]=value;input.checked=value;checkbox.__vue__.value=value;}}}}};line.appendChild(checkbox);}
    editor.$el.appendChild(line);
  }const submit=document.createElement('button');submit.textContent='提交';editor.$el.appendChild(submit);}
  const show=detail.show.bind(detail);
  detail.show=function(row){show(row);if(row.saLzkId==='smoke-metadata'){
    const author=saved.metadata.author[1];const info=this.compareData.find(f=>f.label==='作者信息');info.compareRightValue=`署名：Tester<br/>工号：001<br/>是否第一作者：${author.commonFirst?'是':'否'}<br/>是否通讯作者：${author.correspondent?'是':'否'}`;
    this.saLzkCompareData=copy(this.compareData);
  }};
})();
