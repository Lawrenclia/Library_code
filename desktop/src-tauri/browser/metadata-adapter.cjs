/* Grounded in ItemEdit (wrapper) and itemModelEdit (module 7013), publicly
 * served in chunk-92b4b496.99ffa744.js on 2026-10-08. Only operates the
 * page's author checkbox bindings, onSubmit and getItemDetailData methods.
 * No REST, credentials, hidden author flags or inferred institution IDs. */
async function runMetadataCommand(command) {
  let submitted = false, restoreSubmit;
  const fail = (code, message) => { throw Object.assign(new Error(message), {code}); };
  const copy = value => JSON.parse(JSON.stringify(value));
  const canonical = value => Array.isArray(value) ? value.map(canonical) : value && typeof value === 'object'
    ? Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])])) : value;
  const equal = (a, b) => JSON.stringify(canonical(a)) === JSON.stringify(canonical(b));
  const visible = element => !!element && element.isConnected && element.getClientRects().length > 0 && getComputedStyle(element).visibility !== 'hidden';
  const panels = () => [...document.querySelectorAll('.el-dialog, .el-drawer, .el-message-box')].filter(visible);
  const one = (rows, label) => { if (rows.length !== 1) fail('PAGE_UNSUPPORTED', `${label}未唯一定位。`); return rows[0]; };
  const check = () => {
    const u = new URL(location.href);
    if (u.hostname !== 'admin.ir.lib.sjtu.edu.cn' || !['http:', 'https:'].includes(u.protocol) || u.hash.split('?')[0] !== '#/dataCompare/list')
      fail('AUTH_REQUIRED', '请返回 SA 比对页并完成机构登录。');
    if (!Number.isFinite(command.expires) || Date.now() >= command.expires - 2500)
      fail(submitted ? 'REMOTE_RESULT_UNKNOWN' : 'PAGE_TIMEOUT', '编辑步骤到时，请先回读结果。');
  };
  const wait = async (predicate, label) => {
    const end = Math.min(Date.now() + 30000, command.expires - 3000);
    while (Date.now() < end) { check(); if (predicate()) return; await new Promise(resolve => setTimeout(resolve, 100)); }
    fail(submitted ? 'REMOTE_RESULT_UNKNOWN' : 'PAGE_TIMEOUT', `${label}未完成。`);
  };
  const textId = (value, label) => {
    if (typeof value !== 'string' || !value.trim() || value.length > 200 || /[,\s\u0000-\u001f]/.test(value))
      fail('IDENTITY_CONFLICT', `${label}不是完整文本编号。`);
    return value;
  };
  const components = () => {
    const found = new Set();
    const visit = vm => { if (!vm || found.has(vm)) return; found.add(vm); (vm.$children || []).forEach(visit); };
    [...document.querySelectorAll('*')].forEach(element => visit(element.__vue__));
    return [...found];
  };
  try {
    check();
    if (!['metadata_read', 'metadata_save', 'metadata_check', 'metadata_close'].includes(command.action)) fail('INVALID_ACTION', '未知条目编辑步骤。');
    textId(command.sa_id, 'SA ID'); textId(command.item_id, '条目 ID');
    if (!/^[0-9]{1,40}$/.test(command.staff_id || '')) fail('IDENTITY_CONFLICT', '需要完整文本工号。');
    const vm = one(components().filter(v => v.$options?.name === 'dataCompare'), 'SA 比对组件');
    const detail = vm.$refs?.compareDetailDrawer;
    const owner = detail?.$refs?.itemEdit;
    if (!owner || typeof owner.handleCloses !== 'function') fail('PAGE_UNSUPPORTED', '未识别到条目编辑容器。');
    if (!owner.drawer && command.action === 'metadata_close') return {ok: true, data: {closed: true}};
    if (!owner.drawer && command.action === 'metadata_check') fail('EDITOR_NOT_OPEN', '需要重新打开本条目的只读编辑表单来核验。');
    if (!owner.drawer || !owner.shows || owner.itemId !== command.item_id || owner.__libraryTask !== command.sa_id)
      fail('REVIEW_REQUIRED', '编辑页不是程序为本条任务打开的页面，请先人工处理未保存内容。');
    await wait(() => owner.$refs?.itemModelEdit && !owner.$refs.itemModelEdit.loading, '加载条目编辑表单');
    const editor = owner.$refs.itemModelEdit;
    if (editor.$options?.name !== 'itemModelEdit' || !['getItemDetailData', 'onSubmit', 'submitData'].every(k => typeof editor[k] === 'function') || typeof editor.$nextTick !== 'function')
      fail('PAGE_UNSUPPORTED', '条目编辑组件结构已变化。');
    const guard = () => {
      check();
      if (owner.itemId !== command.item_id || owner.__libraryTask !== command.sa_id || !owner.drawer || owner.$refs.itemModelEdit !== editor || editor.itemId !== command.item_id)
        fail('IDENTITY_CONFLICT', '编辑期间任务或条目目标已变化。');
      if (editor.auth || !visible(editor.$el)) fail('AUTH_REQUIRED', '没有可用的条目编辑权限。');
      if (!detail.dialogVisible || detail.currentSaLzkId !== command.sa_id || vm.loading || detail.dialogLoading)
        fail('TASK_CHANGED', 'SA 详情目标变化或仍在加载。');
      const row = one((vm.tableData || []).filter(row => row.saLzkId === command.sa_id), 'SA 记录');
      if (Number(row.matchCount) !== 1 || String(row.itemId || '').replace(/^,/, '') !== command.item_id || row.gh !== command.staff_id || row.markStatus !== '待处理')
        fail('IDENTITY_CONFLICT', 'SA 没有返回同一工号和唯一待处理条目。');
      if (!command.expected_row || !Object.keys(command.expected_row).every(key => equal(row[key] ?? '', command.expected_row[key])))
        fail('TASK_CHANGED', 'SA 字段在准备后发生变化。');
      if (panels().some(panel => !editor.$el.contains(panel) && !owner.$el?.contains(panel) && !detail.$el?.contains(panel)))
        fail('REVIEW_REQUIRED', '存在其他编辑或确认窗口。');
    };
    const snapshot = () => {
      const form = editor.ruleForm;
      if (!form || form.id !== command.item_id || !form.metadata || !Array.isArray(form.metadata.title) || !form.metadata.title.length ||
          form.metadata.title.some(title => typeof title !== 'string' || !title.trim()) || !Array.isArray(form.metadata.author) ||
          !Array.isArray(editor.modelFieldList) || !editor.validInfo || !Array.isArray(form.fullTexts))
        fail('PAGE_UNSUPPORTED', '编辑表单的条目、题名、作者或附件字段结构未知。');
      for (const author of form.metadata.author) {
        textId(author.id, '作者 ID');
        if (typeof author.fullname !== 'string' || !author.fullname.trim() || (author.scholarId != null && typeof author.scholarId !== 'string'))
          fail('IDENTITY_CONFLICT', '作者署名或学者编号格式未知。');
      }
      if (new Set(form.metadata.author.map(a => a.id)).size !== form.metadata.author.length) fail('IDENTITY_CONFLICT', '作者 ID 重复。');
      // Capture only the actual form and model fields, never editor.header/store.
      return {form: copy(form), fields: copy(editor.modelFieldList)};
    };
    const clean = snapshot(); guard();
    if (owner.__libraryClean && !equal(clean, owner.__libraryClean)) {
      if (command.action !== 'metadata_check' || !owner.__libraryExpected || !sameContent(clean.form, owner.__libraryExpected))
        fail('REVIEW_REQUIRED', '编辑页有未保存或未经回读的变化，不能覆盖或关闭。');
    }
    if (command.action === 'metadata_close') {
      if (!owner.__libraryClean) fail('REVIEW_REQUIRED', '编辑页尚未建立只读快照，不能自动关闭。');
      owner.handleCloses(); await editor.$nextTick();
      await wait(() => !owner.drawer && !owner.shows, '关闭已核对的编辑页');
      return {ok: true, data: {closed: true}};
    }
    if (typeof command.scholar?.id !== 'string' || !command.scholar.id || command.scholar.wno !== command.staff_id ||
        !Array.isArray(command.names) || !command.names.length || command.names.some(n => typeof n !== 'string' || !n.trim()))
      fail('IDENTITY_CONFLICT', '缺少按完整工号读取的真实学者身份与别名。');
    const eligible = author => (author.scholarId === command.scholar.id || command.names.includes(author.fullname.trim())) &&
      (!author.scholarId || author.scholarId === command.scholar.id);
    const control = (author, index, field) => {
      const schema = one(editor.modelFieldList.filter(f => f.fieldName === 'author'), '作者字段定义');
      if (schema.reveal !== 1 || schema.dataType !== 4 || schema.editable === 0 || schema.customConfig?.[field] !== true ||
          (field === 'commonFirst' && index === 0)) fail('PAGE_UNSUPPORTED', '当前模型没有对应的可编辑作者控件。');
      const inputs = components().filter(v => v.$options?.name === 'ElInput' && v.$vnode?.data?.model?.expression === 'val.fullname' &&
        editor.$el.contains(v.$el) && visible(v.$el) && v.value === author.fullname);
      const nameInput = one(inputs, '目标作者署名输入框');
      const row = nameInput.$el.closest('.el-form-item');
      if (!row || !editor.$el.contains(row)) fail('PAGE_UNSUPPORTED', '目标作者行结构已变化。');
      const checkbox = one(components().filter(v => v.$options?.name === 'ElCheckbox' && v.$vnode?.data?.model?.expression === `val.${field}` &&
        row.contains(v.$el) && visible(v.$el)), '目标作者角色控件');
      const input = one([...checkbox.$el.querySelectorAll('input[type="checkbox"]')].filter(visible), '角色复选框');
      if (checkbox.disabled || input.disabled || typeof checkbox.$vnode.data.model.callback !== 'function' || typeof author[field] !== 'boolean' ||
          checkbox.value !== author[field] || input.checked !== author[field]) fail('PAGE_UNSUPPORTED', '角色控件不可编辑或绑定值不一致。');
      return checkbox;
    };
    if (command.action === 'metadata_read') {
      if (owner.__libraryClean && !equal(clean, owner.__libraryClean)) fail('REVIEW_REQUIRED', '尚未核验上次表单变化。');
      owner.__libraryClean = clean;
      const authors = clean.form.metadata.author.map((author, index) => {
        const fields = [];
        if (eligible(author)) for (const field of ['correspondent', 'commonFirst']) {
          try {control(author, index, field); fields.push(field);} catch (error) {if (error.code !== 'PAGE_UNSUPPORTED') throw error;}
        }
        return {index, id: author.id, fullname: author.fullname, scholar_id: author.scholarId || '', order: author.order,
          eligible: eligible(author), fields, correspondent: author.correspondent, commonFirst: author.commonFirst,
          ownFirst: author.ownFirst, ownCorrespondent: author.ownCorrespondent, commonCorrespondent: author.commonCorrespondent};
      });
      return {ok: true, data: {item_id: command.item_id, staff_id: command.staff_id, scholar: copy(command.scholar), authors, snapshot: clean}};
    }
    const index = command.author_index;
    if (!Number.isSafeInteger(index) || index < 0 || index >= clean.form.metadata.author.length || typeof command.value !== 'boolean')
      fail('REVIEW_REQUIRED', '请明确选择目标作者和核实后的角色值。');
    const author = editor.ruleForm.metadata.author[index];
    if (!eligible(author) || author.id !== command.author_id || author.fullname !== command.fullname)
      fail('IDENTITY_CONFLICT', '选中作者与核对后的学者身份不一致。');
    const field = command.key === 'corresponding_author' ? 'correspondent' : command.key === 'first_author' ? 'commonFirst' : '';
    if (!field) fail('PAGE_UNSUPPORTED', '此原因尚需适配实际顺序控件，不能直接改隐藏字段。');
    const expected = command.expected_snapshot;
    if (!expected || expected.form?.id !== command.item_id || !Array.isArray(expected.form?.metadata?.author) ||
        expected.form.metadata.author[index]?.id !== command.author_id || expected.form.metadata.author[index]?.fullname !== command.fullname)
      fail('IDENTITY_CONFLICT', '缺少目标作者的完整编辑前快照。');
    const wanted = copy(expected.form); wanted.metadata.author[index][field] = command.value;
    if (command.action === 'metadata_check') {
      // A read-only recovery may reload a program-owned pending form, but never
      // a user-modified draft; an in-flight save is not retried or cancelled.
      if (!equal(clean, owner.__libraryClean) && !sameContent(clean.form, wanted)) fail('REVIEW_REQUIRED', '回读前存在人工编辑。');
      editor.loading = true;
      const previous = editor.ruleForm;
      const previousFields = editor.modelFieldList;
      let done = false, failed = false;
      const request = editor.getItemDetailData();
      if (!request || typeof request.then !== 'function') fail('PAGE_UNSUPPORTED', '条目回读方法不兼容。');
      request.then(() => done = true, () => {done = true; failed = true;});
      await wait(() => done && !editor.loading && editor.ruleForm !== previous && editor.modelFieldList !== previousFields, '从平台重新读取条目'); guard();
      const after = snapshot();
      if (failed || !equal(after.fields, expected.fields) || !sameContent(after.form, wanted)) fail('REMOTE_RESULT_UNKNOWN', '平台没有回读到本次完整修改结果；不能重复提交。');
      owner.__libraryClean = after; delete owner.__libraryExpected;
      return {ok: true, data: {verified: true, item_id: command.item_id, staff_id: command.staff_id, author_id: command.author_id, key: command.key, value: command.value, before: expected.form, after: after.form}};
    }
    if (command.confirmed !== true || !equal(clean, expected) || !equal(clean, owner.__libraryClean)) fail('TASK_CHANGED', '完整表单在核对后变化或缺少本条确认，请重新读取。');
    if (!command.value && ((field === 'correspondent' && (author.ownCorrespondent === true || author.commonCorrespondent === true)) ||
        (field === 'commonFirst' && (author.order === 1 || author.ownFirst === true))))
      fail('REVIEW_REQUIRED', '角色由其他标记或署名顺序确定，当前复选框不能完成这项修正。');
    const checkbox = control(author, index, field);
    if (author[field] === command.value) return {ok: true, data: {verified: true, already_present: true, item_id: command.item_id, staff_id: command.staff_id, author_id: author.id, key: command.key, value: command.value, before: clean.form, after: clean.form}};
    checkbox.$vnode.data.model.callback(command.value);
    await editor.$nextTick(); guard();
    if (!equal(snapshot(), {form: wanted, fields: expected.fields}) || author[field] !== command.value) fail('TASK_CHANGED', '角色更新带来其他表单变化，请人工核对。');
    const button = one([...editor.$el.querySelectorAll('button')].filter(b => visible(b) && !b.disabled && b.textContent.trim() === '提交'), '条目提交按钮');
    if (!visible(button) || editor.loading) fail('PAGE_TIMEOUT', '条目仍在加载。');
    const original = editor.submitData;
    let started = false, done = false;
    const wrapped = function (...args) {
      if (started || args[0] !== editor.ruleForm) fail('PAGE_UNSUPPORTED', '提交入口不一致。');
      guard();
      if (!equal(snapshot(), {form: wanted, fields: expected.fields})) fail('TASK_CHANGED', '提交前完整表单已变化。');
      started = true; submitted = true;
      const request = original.apply(this, args);
      if (request && typeof request.then === 'function') request.then(() => done = true, () => done = true);
      return request;
    };
    editor.submitData = wrapped;
    restoreSubmit = () => {if (editor.submitData === wrapped) editor.submitData = original;};
    owner.__libraryExpected = wanted;
    const editing = editor.ruleForm;
    const editingFields = editor.modelFieldList;
    const result = editor.onSubmit();
    if (!started) {
      checkbox.$vnode.data.model.callback(expected.form.metadata.author[index][field]);
      await editor.$nextTick(); delete owner.__libraryExpected;
      fail('INCOMPLETE_METADATA', result === false ? '平台表单校验未通过，没有提交写入。' : '提交方法没有调用已确认的保存入口。');
    }
    await wait(() => done && !editor.loading && editor.ruleForm !== editing && editor.modelFieldList !== editingFields, '保存后重新读取条目'); guard();
    const after = snapshot();
    if (!equal(after.fields, expected.fields) || !sameContent(after.form, wanted)) fail('REMOTE_RESULT_UNKNOWN', '角色或其他元数据回读不一致，先核验实际结果。');
    owner.__libraryClean = after; delete owner.__libraryExpected;
    return {ok: true, data: {verified: true, item_id: command.item_id, staff_id: command.staff_id, author_id: author.id, key: command.key, value: command.value, before: clean.form, after: after.form}};
  } catch (error) {
    return {ok: false, code: error.code || 'PAGE_UNSUPPORTED', submitted, error: error.message};
  } finally {restoreSubmit?.();}
  function sameContent(a, b) {
    // submitData converts institution order input strings to arrays. The editor
    // converts them back on fresh load. Treat only this documented encoding as
    // equivalent; preserve order, empty slots and every other metadata field.
    const content = form => {
      const metadata = copy(form.metadata);
      for (const author of metadata.author || []) if (Array.isArray(author.institutionOrderNums)) author.institutionOrderNums = author.institutionOrderNums.join(',');
      return {id: form.id, modelId: form.modelId, modelName: form.modelName, datasetIds: form.datasetIds, dataSources: form.dataSources, fullTexts: form.fullTexts, metadata};
    };
    return equal(content(a), content(b));
  }
}
if (typeof module !== 'undefined') module.exports = {runMetadataCommand};
