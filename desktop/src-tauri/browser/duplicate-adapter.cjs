/* Grounded in duplicateData.vue (e994), CitationModal.vue (5b22),
 * ItemDetail.vue (ca30) and Detail.vue (5358), publicly served on 2026-10-08.
 * Executes the visible page's own component/UI actions. No REST or credentials. */
async function runDuplicateCommand(command) {
  let submitted = false, offSuccess;
  const fail = (code, message) => { throw Object.assign(new Error(message), {code}); };
  const norm = v => String(v ?? '').normalize('NFKC').trim().toLowerCase().replace(/\s+/g, ' ');
  const visible = el => !!el && el.getClientRects().length > 0 && getComputedStyle(el).visibility !== 'hidden';
  const panels = () => [...document.querySelectorAll('.el-dialog, .el-drawer, .el-message-box')].filter(visible);
  const one = (rows, label) => { if (rows.length !== 1) fail('PAGE_UNSUPPORTED', `${label}不是唯一对象。`); return rows[0]; };
  const canonical = v => Array.isArray(v) ? v.map(canonical) : v && typeof v === 'object'
    ? Object.fromEntries(Object.keys(v).sort().map(k => [k, canonical(v[k])])) : v;
  const equal = (a,b) => JSON.stringify(canonical(a)) === JSON.stringify(canonical(b));
  const textId = (id, label) => {
    if (typeof id !== 'string' || !id.trim() || id.length > 200 || /[,\s\u0000-\u001f]/.test(id)) fail('IDENTITY_CONFLICT', `${label}不是完整文本编号。`);
    return id;
  };
  const check = () => {
    const u = new URL(location.href);
    if (u.hostname !== 'admin.ir.lib.sjtu.edu.cn' || !['http:', 'https:'].includes(u.protocol) || u.hash.split('?')[0] !== '#/collectItem/duplicateData')
      fail('AUTH_REQUIRED', '请在重复数据管理页完成机构登录。');
    if (!Number.isFinite(command.expires) || Date.now() >= command.expires - 2500)
      fail(submitted ? 'REMOTE_RESULT_UNKNOWN' : 'PAGE_TIMEOUT', '重复条目操作到时，请先回读结果。');
  };
  const wait = async (predicate, label) => {
    const end = Math.min(Date.now() + 30000, command.expires - 3000);
    while (Date.now() < end) {check(); if (predicate()) return; await new Promise(r => setTimeout(r, 100));}
    fail(submitted ? 'REMOTE_RESULT_UNKNOWN' : 'PAGE_TIMEOUT', `${label}未完成。`);
  };
  const item = raw => {
    textId(raw?.id, '条目 ID');
    if (typeof raw.modelName !== 'string' || !raw.modelName || !raw.metadata || !Array.isArray(raw.metadata.title) ||
        !raw.metadata.title.length || raw.metadata.title.some(t => typeof t !== 'string' || !t.trim()))
      fail('PAGE_UNSUPPORTED', '重复条目的题名或模型字段不兼容。');
    return {id: raw.id, model_name: raw.modelName, metadata: JSON.parse(JSON.stringify(raw.metadata))};
  };
  const group = raw => {
    textId(raw?.id, '候选组 ID');
    const primary = item(raw.item);
    if (!Array.isArray(raw.items)) fail('PAGE_UNSUPPORTED', '候选组缺少条目列表。');
    const items = [primary, ...raw.items.map(r => item(r.item))];
    if (new Set(items.map(r => r.id)).size !== items.length) fail('IDENTITY_CONFLICT', '候选组存在重复编号。');
    return {id: raw.id, primary_id: primary.id, items};
  };
  try {
    check();
    if (!['duplicate_scan','duplicate_read','duplicate_merge','duplicate_check'].includes(command.action)) fail('INVALID_ACTION','未知重复条目操作。');
    textId(command.sa_id, 'SA ID');
    if (typeof command.title !== 'string' || !command.title.trim()) fail('REVIEW_REQUIRED','缺少核对后的检索题名。');
    const vms = new Set();
    const visit = vm => {if (!vm || vms.has(vm)) return; vms.add(vm); (vm.$children || []).forEach(visit);};
    [...document.querySelectorAll('*')].forEach(el => visit(el.__vue__));
    const vm = one([...vms].filter(v => v.$refs?.citationModal && v.queryForm && Object.hasOwn(v.queryForm,'majorTitle') && v.page && Object.hasOwn(v.page,'titleSimilarity')), '重复管理组件');
    const modal = vm.$refs.citationModal;
    if (!['getData','details'].every(k=>typeof vm[k]==='function') || !Array.isArray(vm.tableData) ||
        !['show','getList','zhuTiaoMu','mergeas'].every(k=>typeof modal[k]==='function') ||
        typeof modal.$on !== 'function' || typeof modal.$off !== 'function') fail('PAGE_UNSUPPORTED','重复管理或详情窗口结构已变化。');
    const owns = el => modal.$el?.contains(el) || modal.$refs?.detail?.$el?.contains(el);
    const closeOwned = async () => {
      if (panels().some(el => !owns(el))) fail('REVIEW_REQUIRED','存在其他编辑或确认窗口，请先人工处理。');
      if ((modal.drawer || modal.$refs?.detail?.drawer) && modal.__libraryTask !== command.sa_id) fail('REVIEW_REQUIRED','存在人工打开的详情窗口。');
      if (modal.$refs?.recordModal?.drawer) fail('REVIEW_REQUIRED','存在字段比较窗口，请先核对并关闭。');
      modal.$refs?.detail?.handleCloses?.(); modal.drawer = false;
      await vm.$nextTick();
      await wait(()=>panels().length === 0,'关闭已读取的详情');
    };
    await closeOwned();
    const threshold = Number(vm.page.titleSimilarity);
    if (!Number.isFinite(threshold) || threshold < 85 || threshold > 100) fail('PAGE_UNSUPPORTED','题名相似度控件值无效。');
    if (command.expected_threshold != null && threshold !== Number(command.expected_threshold)) fail('TASK_CHANGED','页面检索条件在读取后发生变化。');
    const query = {majorModelId:'',majorTitle:command.title,sameAbstract:'',sameAuthor:'',sameDoi:'',sameIssue:'',sameIssuedYear:'',sameModelId:'',samePages:'',sameSource:'',sameVolume:'',sameUniqueIdentificationField:''};
    const groupPages = new Map();
    const scan = async (allowOwned = false) => {
      if (vm.loading) fail('PAGE_TIMEOUT','重复列表仍在加载。');
      vm.queryForm={...query};vm.queryFormShow=true;vm.page.current=1;vm.page.size=50;
      groupPages.clear();const found=[],seen=new Set();let total;
      for (let page=1;page<=4;page++) {
        check();if (panels().length && (!allowOwned || panels().some(el=>!owns(el)))) fail('REVIEW_REQUIRED','检索期间出现其他窗口。');
        const previous=vm.tableData;vm.page.current=page;
        const request=vm.getData(vm.queryForm);
        if (!request || typeof request.then!=='function') fail('PAGE_UNSUPPORTED','重复检索方法不兼容。');
        let done=false,failed=false;request.then(()=>done=true,()=>{done=true;failed=true;});
        await wait(()=>done&&!vm.loading,'检索重复候选');
        if (failed || vm.tableData===previous || !Array.isArray(vm.tableData) || !Number.isInteger(Number(vm.total)) || Number(vm.total)<0)
          fail('PAGE_UNSUPPORTED','重复检索没有返回新的完整列表。');
        if (vm.page.current!==page || vm.page.size!==50 || !equal(vm.queryForm,query) || Number(vm.page.titleSimilarity)!==threshold) fail('TASK_CHANGED','检索对象或条件已变化。');
        if (total==null)total=Number(vm.total);else if(total!==Number(vm.total))fail('TASK_CHANGED','候选数量在分页期间发生变化。');
        if(total>200)fail('AMBIGUOUS_RESULT','候选超过 200 组，请人工缩小检索范围；没有只读取第一页。');
        for(const raw of vm.tableData){const g=group(raw);if(seen.has(g.id))fail('TASK_CHANGED','分页返回重复候选组。');seen.add(g.id);found.push(g);groupPages.set(g.id,page);}
        if(found.length>=total){if(found.length!==total)fail('PAGE_UNSUPPORTED','候选总数不一致。');return found;}
        if(!vm.tableData.length)fail('PAGE_UNSUPPORTED','分页缺少候选数据。');
      }
      fail('AMBIGUOUS_RESULT','尚未读取全部候选组。');
    };
    const groups=await scan();
    if(command.action==='duplicate_scan')return {ok:true,data:{groups,title:command.title,title_similarity:threshold,query}};
    const readMaster=async master=>{
      if(panels().some(el=>!owns(el)))fail('REVIEW_REQUIRED','主条目回读前出现其他窗口。');
      modal.__libraryTask=command.sa_id;
      // CitationModal uses destroy-on-close, so keep its parent mounted while
      // reading its ItemDetail child. An empty shell is never absence evidence.
      if(!modal.drawer){modal.list={items:[]};modal.title='主条目回读';modal.drawer=true;await vm.$nextTick();}
      if(!modal.list||Array.isArray(modal.list))modal.list={items:[]};
      const detail=modal.$refs?.detail;
      if(!detail||typeof detail.show!=='function'||typeof detail.handleCloses!=='function')fail('PAGE_UNSUPPORTED','主条目详情窗口结构已变化。');
      if(detail.drawer){detail.handleCloses();await vm.$nextTick();}
      detail.show(master.id);
      await wait(()=>detail.drawer&&detail.shows&&detail.id===master.id&&detail.$refs?.detail,'打开主条目详情');
      const inner=detail.$refs.detail;
      if(typeof inner.getItemDetail!=='function'||!Array.isArray(inner.resultList))fail('PAGE_UNSUPPORTED','主条目详情结构不兼容。');
      // Wait out show()'s initial request, then explicitly clear and re-read so
      // an old successful record cannot masquerade as the requested master.
      await wait(()=>!inner.loading,'首次主条目读取');
      inner.resultList=[];inner.status=null;inner.auth=true;
      const previous=inner.resultList;let done=false,failed=false;
      const request=inner.getItemDetail();
      if(!request||typeof request.then!=='function')fail('PAGE_UNSUPPORTED','主条目读取方法不兼容。');
      request.then(()=>done=true,()=>{done=true;failed=true;});
      await wait(()=>done&&!inner.loading,'回读主条目');
      if(failed||inner.auth!==true||inner.id!==master.id||detail.id!==master.id||inner.resultList===previous||!inner.resultList.length)
        fail('REMOTE_RESULT_UNKNOWN','未取得主条目的新详情或没有访问权限。');
      const fields=Object.create(null);
      for(const f of inner.resultList){if(typeof f.fieldName!=='string'||!Object.hasOwn(f,f.fieldName)||Object.hasOwn(fields,f.fieldName))fail('PAGE_UNSUPPORTED','主条目字段结构未知。');fields[f.fieldName]=JSON.parse(JSON.stringify(f[f.fieldName]));}
      if(!Array.isArray(fields.title)||!fields.title.some(t=>master.metadata.title.some(old=>norm(old)===norm(t))))fail('IDENTITY_CONFLICT','主条目回读题名与核验目标不符。');
      if(panels().some(el=>!owns(el))||!detail.drawer)fail('REVIEW_REQUIRED','回读期间出现其他窗口。');
      return {id:master.id,model_name:inner.modelName,fields,status:inner.status};
    };
    if(command.action==='duplicate_check'){
      textId(command.source_id,'被合并 ID');textId(command.target_id,'主条目 ID');
      if(command.source_id===command.target_id||command.sa_verified!==true||command.expected_master?.id!==command.target_id)fail('REVIEW_REQUIRED','需要先回读本次合并后的 SA 条目集合。');
      if(!Array.isArray(command.sa_ids)||!Array.isArray(command.expected_sa_ids))fail('REVIEW_REQUIRED','缺少合并前后 SA 编号核对。');
      const current=command.sa_ids.map(i=>textId(i,'实时 SA 条目')).sort(),expected=command.expected_sa_ids.map(i=>textId(i,'原 SA 条目'));
      if(new Set(current).size!==current.length||new Set(expected).size!==expected.length||!expected.includes(command.source_id)||!expected.includes(command.target_id)||!equal(current,expected.filter(i=>i!==command.source_id).sort()))fail('REMOTE_RESULT_UNKNOWN','SA 条目集合与本次合并不一致。');
      if(groups.some(g=>g.items.some(i=>i.id===command.source_id)))fail('REMOTE_RESULT_UNKNOWN','被合并条目仍在候选池，不能重复合并。');
      const master=await readMaster(command.expected_master);
      return {ok:true,data:{verified:true,verification:'fresh_sa_and_pool_and_master',source_id:command.source_id,target_id:command.target_id,master_after:master,pool_after:groups}};
    }
    const selected=one(groups.filter(g=>g.id===command.group_id),'选定候选组');
    if(command.expected_group&&!equal(command.expected_group,selected))fail('TASK_CHANGED','重复条目字段在读取后发生变化。');
    // If the selected group belongs to a preceding page, locate it through
    // the same paged UI query; never call an undiscovered backend endpoint.
    const selectedPage=groupPages.get(selected.id);
    if(vm.page.current!==selectedPage){
      const previous=vm.tableData;vm.page.current=selectedPage;
      let done=false,failed=false;const request=vm.getData(vm.queryForm);
      if(!request||typeof request.then!=='function')fail('PAGE_UNSUPPORTED','候选分页方法不兼容。');
      request.then(()=>done=true,()=>{done=true;failed=true;});
      await wait(()=>done&&!vm.loading,'定位候选所在页');
      if(failed||vm.tableData===previous||Number(vm.total)!==groups.length||vm.page.current!==selectedPage||!equal(vm.queryForm,query)||Number(vm.page.titleSimilarity)!==threshold)fail('TASK_CHANGED','候选所在页已变化。');
    }
    const raw=one(vm.tableData.filter(g=>g.id===command.group_id),'当前页候选组');
    if(!equal(group(raw),selected))fail('TASK_CHANGED','候选组在分页后发生变化。');
    const before=modal.list;vm.details(raw);modal.__libraryTask=command.sa_id;
    await wait(()=>modal.drawer&&modal.id===selected.id&&modal.list!==before&&Array.isArray(modal.list?.items),'读取候选组详情');
    const snapshot=()=>{
      const primary=item(modal.list.item),rows=modal.list.items.map(r=>item(r.item));
      if(new Set(rows.map(r=>r.id)).size!==rows.length||!rows.some(r=>r.id===primary.id))fail('IDENTITY_CONFLICT','详情条目编号不完整。');
      return {id:modal.id,primary_id:primary.id,items:rows};
    };
    const guard=()=>{check();if(!modal.drawer||modal.id!==selected.id||!equal(snapshot(),selected)||panels().some(el=>!owns(el)))fail('TASK_CHANGED','候选条目或窗口在核验后变化。');};
    guard();
    if(command.action==='duplicate_read')return {ok:true,data:{group:selected,title:command.title,title_similarity:threshold,query}};
    textId(command.source_id,'被合并 ID');textId(command.target_id,'主条目 ID');
    if(command.confirmed!==true||command.source_id===command.target_id||!command.expected_group)fail('REVIEW_REQUIRED','请选择不同条目并确认本次合并。');
    const source=one(modal.list.items.filter(r=>r.item.id===command.source_id),'被合并条目');
    const target=one(modal.list.items.filter(r=>r.item.id===command.target_id),'主条目');
    let rows=[...modal.$el.querySelectorAll('tr.el-table__row')].filter(visible);
    if(rows.length!==modal.list.items.length||rows.some((r,i)=>!norm(r.textContent).includes(norm(modal.list.items[i].item.metadata.title[0]))))fail('PAGE_UNSUPPORTED','候选列表的可见行不一致。');
    modal.zhuTiaoMu(target.item);await vm.$nextTick();guard();
    if(modal.merge.targetItemId!==command.target_id)fail('IDENTITY_CONFLICT','选定主条目不一致。');
    rows=[...modal.$el.querySelectorAll('tr.el-table__row')].filter(visible);
    if(rows.length!==modal.list.items.length)fail('PAGE_UNSUPPORTED','选择主条目后列表结构变化。');
    const sourceRow=rows[modal.list.items.indexOf(source)];
    const mergeButton=one([...sourceRow.querySelectorAll('button')].filter(b=>visible(b)&&!b.disabled&&b.textContent.trim()==='合并至主条目'),'本行合并按钮');
    let succeeded=false;const success=()=>{succeeded=true;};modal.$on('upData',success);offSuccess=()=>modal.$off('upData',success);
    mergeButton.click();
    await wait(()=>document.querySelectorAll('.el-message-box').length>0,'本条合并确认');
    const box=one([...document.querySelectorAll('.el-message-box')].filter(visible),'合并确认窗口');
    if(box.querySelector('.el-message-box__message')?.textContent.trim()!=='确认合并至主条目？'||modal.merge.itemId!==command.source_id||modal.merge.targetItemId!==command.target_id||!equal(snapshot(),selected))fail('TASK_CHANGED','合并确认内容或对象已变化。');
    if(panels().some(el=>el!==box&&!owns(el)))fail('REVIEW_REQUIRED','出现其他确认或编辑窗口。');
    const confirm=one([...box.querySelectorAll('button')].filter(b=>visible(b)&&!b.disabled&&b.textContent.trim()==='确定'),'合并确定按钮');
    const listBeforeWrite=modal.list;
    check();submitted=true;confirm.click();
    await wait(()=>succeeded,'平台合并成功回执');
    await wait(()=>modal.list!==listBeforeWrite&&!Array.isArray(modal.list),'合并后的候选组刷新');
    const after=await scan(true);
    if(after.some(g=>g.items.some(i=>i.id===command.source_id)))fail('REMOTE_RESULT_UNKNOWN','合并回执已返回，但候选池仍含被合并条目。');
    const expectedMaster=item(target.item);
    expectedMaster.metadata.title=[...new Set([...expectedMaster.metadata.title,...source.item.metadata.title])];
    const master=await readMaster(expectedMaster);
    return {ok:true,data:{verified:true,server_success:true,group_before:selected,source_id:command.source_id,target_id:command.target_id,master_after:master,pool_after:after}};
  }catch(error){return {ok:false,code:error.code||'PAGE_UNSUPPORTED',submitted,error:error.message};}
  finally{offSuccess?.();}
}
if(typeof module!=='undefined')module.exports={runDuplicateCommand};
