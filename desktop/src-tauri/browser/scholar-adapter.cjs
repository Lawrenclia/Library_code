/* Grounded in scholar/list.vue (ade6) and aliasTableModal.vue (2ad7),
 * publicly served in chunk-92b4b496.99ffa744.js on 2026-10-08.
 * Uses the page's visible UI and component methods; no REST or credentials. */
async function runScholarCommand(command) {
  let submitted = false;
  const fail = (code, message) => { throw Object.assign(new Error(message), {code}); };
  const norm = value => String(value ?? '').normalize('NFKC').trim().toLowerCase().replace(/\s+/g, ' ');
  const visible = el => !!el && el.getClientRects().length && getComputedStyle(el).visibility !== 'hidden';
  const panels = () => [...document.querySelectorAll('.el-dialog, .el-drawer, .el-message-box')].filter(visible);
  const check = () => {
    const u = new URL(location.href);
    if (u.hostname !== 'admin.ir.lib.sjtu.edu.cn' || !['http:', 'https:'].includes(u.protocol) || u.hash.split('?')[0] !== '#/scholar/list')
      fail('AUTH_REQUIRED', '请在学者管理页完成登录并确认权限。');
    if (!Number.isFinite(command.expires) || Date.now() >= command.expires - 2500)
      fail(submitted ? 'REMOTE_RESULT_UNKNOWN' : 'PAGE_TIMEOUT', '学者操作到时，请先回读结果。');
  };
  const wait = async (predicate, label) => {
    const end = Math.min(Date.now() + 30000, command.expires - 3000);
    while (Date.now() < end) { check(); if (predicate()) return; await new Promise(r => setTimeout(r, 120)); }
    fail(submitted ? 'REMOTE_RESULT_UNKNOWN' : 'PAGE_TIMEOUT', `${label}未完成。`);
  };
  const one = (rows, label) => { if (rows.length !== 1) fail('PAGE_UNSUPPORTED', `${label}不是唯一对象。`); return rows[0]; };
  const canonical = value => Array.isArray(value) ? value.map(canonical) : value && typeof value === 'object'
    ? Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])])) : value;
  const equal = (a,b) => JSON.stringify(canonical(a)) === JSON.stringify(canonical(b));
  try {
    check();
    if (!['alias_read', 'alias_add', 'alias_check'].includes(command.action)) fail('INVALID_ACTION', '未知学者操作。');
    if (typeof command.sa_id !== 'string' || !command.sa_id || !/^[0-9]{1,40}$/.test(command.staff_id || ''))
      fail('IDENTITY_CONFLICT', '需要当前 SA 任务和完整文本工号。');
    const vms = new Set();
    const visit = vm => { if (!vm || vms.has(vm)) return; vms.add(vm); (vm.$children || []).forEach(visit); };
    [...document.querySelectorAll('*')].forEach(el => visit(el.__vue__));
    const vm = one([...vms].filter(v => v.$options?.name === '学者管理' && v.$refs?.aliasTableModal), '学者管理组件');
    const modal = vm.$refs.aliasTableModal;
    if (!vm.page || !Array.isArray(vm.tableData) || !vm.query || !['getData', 'handleAli'].every(k => typeof vm[k] === 'function') ||
        !['getData', 'handleModal', 'onSubmit'].every(k => typeof modal[k] === 'function') || !Array.isArray(modal.data))
      fail('PAGE_UNSUPPORTED', '学者或别名窗口结构已变化。');
    if (modal.data.some(a => a.isSet)) fail('REVIEW_REQUIRED', '别名窗口有未保存编辑，先人工处理。');
    if (modal.dialogVisible) {
      if (modal.__libraryTask !== command.sa_id || modal.__libraryStaff !== command.staff_id)
        fail('REVIEW_REQUIRED', '存在人工打开的别名窗口。');
      modal.dialogVisible = false;
      await wait(() => panels().length === 0, '关闭只读别名窗口');
    }
    if (panels().length) fail('REVIEW_REQUIRED', '请先关闭其他编辑或确认窗口。');
    if (vm.loading) fail('PAGE_TIMEOUT', '学者列表仍在加载。');
    const previous = vm.tableData;
    vm.query = {wno: command.staff_id}; vm.queryFormShow = true; vm.page.current = 1; vm.page.size = 10;
    let done = false, failed = false;
    const request = vm.getData(vm.query);
    if (!request || typeof request.then !== 'function') fail('PAGE_UNSUPPORTED', '学者查询方法不兼容。');
    request.then(() => done = true, () => {done = true; failed = true;});
    await wait(() => done && !vm.loading, '按工号查询学者');
    if (failed || vm.tableData === previous || Number(vm.page.total) !== 1 || vm.tableData.length !== 1)
      fail('IDENTITY_CONFLICT', '工号没有返回唯一的新学者记录。');
    const row = vm.tableData[0];
    if (typeof row.id !== 'string' || !row.id || typeof row.wno !== 'string' || row.wno !== command.staff_id)
      fail('IDENTITY_CONFLICT', '学者 ID 或工号不是准确文本。');
    const scholar = {id: row.id, wno: row.wno, nameCn: String(row.nameCn || ''), nameEn: String(row.nameEn || '')};
    if (command.expected_scholar && !equal(command.expected_scholar, scholar)) fail('TASK_CHANGED', '学者身份在核验后发生变化。');
    if (command.expected_scholar_id && scholar.id !== command.expected_scholar_id) fail('IDENTITY_CONFLICT', '回读学者目标不一致。');
    const before = modal.data;
    vm.handleAli(row); modal.__libraryTask = command.sa_id; modal.__libraryStaff = command.staff_id;
    await wait(() => modal.dialogVisible && !modal.loading && modal.data !== before, '读取别名');
    const guard = () => {
      check();
      if (!modal.dialogVisible || modal.id !== scholar.id || vm.tableData[0]?.wno !== command.staff_id)
        fail('IDENTITY_CONFLICT', '别名窗口的学者目标已变化。');
      const live = panels();
      if (live.length !== 1 || !modal.$el?.contains(live[0])) fail('REVIEW_REQUIRED', '出现其他窗口，停止别名操作。');
    };
    const snapshot = () => modal.data.map(a => {
      if (typeof a.id !== 'string' || !a.id || typeof a.nameAlias !== 'string' || !a.nameAlias.trim() || a.isSet ||
          (a.scholarId != null && a.scholarId !== scholar.id)) fail('PAGE_UNSUPPORTED', '别名记录格式未知或仍在编辑。');
      return {id: a.id, nameAlias: a.nameAlias, defaultNameCn: a.defaultNameCn ?? 0, defaultNameEn: a.defaultNameEn ?? 0};
    }).sort((a,b) => a.id.localeCompare(b.id));
    guard();
    const aliases = snapshot();
    if (command.action === 'alias_read') return {ok: true, data: {scholar, aliases, staff_id: command.staff_id}};
    if (typeof command.alias !== 'string' || !command.alias.trim() || command.alias.length > 200 || /[\r\n\u0000-\u001f]/.test(command.alias))
      fail('REVIEW_REQUIRED', '别名须为来源中的单个真实署名。');
    const found = aliases.filter(a => norm(a.nameAlias) === norm(command.alias));
    if (found.length > 1) fail('IDENTITY_CONFLICT', '同名别名记录不唯一，请人工核对。');
    if (command.action === 'alias_check') {
      if (found.length !== 1) fail('REMOTE_RESULT_UNKNOWN', '尚未回读到目标别名，不能重复新增。');
      return {ok: true, data: {verified: true, scholar, aliases, alias: found[0]}};
    }
    if (command.confirmed !== true || !equal(aliases, command.expected_aliases)) fail('TASK_CHANGED', '别名列表或本条确认已变化，请重新读取。');
    if (found.length === 1) return {ok: true, data: {verified: true, already_present: true, scholar, aliases, alias: found[0]}};
    const add = [...modal.$el.querySelectorAll('button')].filter(b => visible(b) && b.textContent.trim() === '新增' && !b.disabled);
    one(add, '别名新增按钮');
    modal.handleModal('add');
    const fresh = one(modal.data.filter(a => a.isSet && a.id === '' && a.scholarId === scholar.id), '新增别名行');
    fresh.nameAlias = command.alias.trim();
    await vm.$nextTick(); guard();
    const save = [...modal.$el.querySelectorAll('button')].filter(b => visible(b) && b.textContent.trim() === '保存' && !b.disabled);
    one(save, '别名保存按钮');
    if (fresh.defaultNameCn !== 0 || fresh.defaultNameEn !== 0) fail('REVIEW_REQUIRED', '新增别名不得改变默认姓名。');
    const editing = modal.data;
    submitted = true; modal.onSubmit(fresh);
    await wait(() => !modal.loading && modal.data !== editing, '保存并回读别名'); guard();
    const after = snapshot(), saved = one(after.filter(a => norm(a.nameAlias) === norm(command.alias)), '已保存的别名');
    if (!aliases.every(old => after.some(a => equal(a, old))) || after.length !== aliases.length + 1)
      fail('REMOTE_RESULT_UNKNOWN', '保存期间其他别名发生变化，请核对实际结果。');
    return {ok: true, data: {verified: true, scholar, aliases: after, alias: saved}};
  } catch (error) {
    return {ok: false, code: error.code || 'PAGE_UNSUPPORTED', submitted, error: error.message};
  }
}
if (typeof module !== 'undefined') module.exports = {runScholarCommand};
