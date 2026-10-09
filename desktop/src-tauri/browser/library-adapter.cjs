/* Public front-end: /advancedSearch, cf49 in chunk-6c2ca03d.5a582740.js,
 * inspected 2026-10-09. Uses searchModal.submitForm, advancedSearch.advanceSubmit
 * and the page's searchItemList event. No API calls, tokens or account state. */
async function runLibraryCommand(command) {
  const fail = (code, message) => { throw Object.assign(new Error(message), {code}); };
  const clone = v => JSON.parse(JSON.stringify(v));
  const canonical = v => Array.isArray(v) ? v.map(canonical) : v && typeof v === 'object'
    ? Object.fromEntries(Object.keys(v).sort().map(k => [k, canonical(v[k])])) : v;
  const equal = (a, b) => JSON.stringify(canonical(a)) === JSON.stringify(canonical(b));
  const visible = e => !!e && e.getClientRects().length > 0 && getComputedStyle(e).visibility !== 'hidden';
  const norm = v => String(v ?? '').normalize('NFKC').trim().toLowerCase().replace(/[\s_-]+/g, '');
  const one = (rows, label) => { if (rows.length !== 1) fail('PAGE_UNSUPPORTED', `${label}未唯一识别。`); return rows[0]; };
  let bus, listener;
  const check = () => {
    const url = new URL(location.href);
    if (url.hostname !== 'www.ir.lib.sjtu.edu.cn' || !['http:', 'https:'].includes(url.protocol) ||
        url.username || url.password || !['/advancedSearch', '/advancedSearch/'].includes(url.pathname))
      fail('AUTH_REQUIRED', '请打开机构库前端高级检索页，完成机构访问与登录。');
    if (!Number.isFinite(command.expires) || Date.now() >= command.expires - 2000)
      fail('PAGE_TIMEOUT', '本库查询超时；没有确认查询结果。');
  };
  const wait = async predicate => {
    while (!predicate()) { check(); await new Promise(r => setTimeout(r, 80)); }
    check();
  };
  try {
    check();
    if (command.action !== 'library_search' || typeof command.sa_id !== 'string' || !command.sa_id ||
        typeof command.title !== 'string' || !command.title.trim() || command.title.length > 1000 || /[\x00-\x1f]/.test(command.title))
      fail('IDENTITY_CONFLICT', '需要当前任务和完整、正确题名。');
    const vms = new Set();
    const visit = v => { if (!v || vms.has(v)) return; vms.add(v); (v.$children || []).forEach(visit); };
    [...document.querySelectorAll('*')].forEach(e => visit(e.__vue__));
    const vm = one([...vms].filter(v => v.$options?.name === 'advancedSearch' && visible(v.$el)), '高级检索组件');
    const modal = vm.$refs?.searchModalRef;
    if (!modal || modal.$options?.name !== 'searchModal' || !visible(modal.$el) ||
        typeof modal.submitForm !== 'function' || typeof vm.advanceSubmit !== 'function' ||
        !modal.$refs?.ruleForm || !Array.isArray(modal.retrievalFieldList) ||
        !vm.ruleForm || !modal.ruleForm || !Array.isArray(vm.ruleForm.queryFields))
      fail('PAGE_UNSUPPORTED', '前端检索表单结构已变化。');
    if ([...document.querySelectorAll('.ant-modal, .ant-drawer, .el-dialog, .el-drawer')].some(visible))
      fail('REVIEW_REQUIRED', '请先处理或关闭现有编辑窗口。');
    if (vm.loading || modal.loading) fail('PAGE_TIMEOUT', '页面已有查询正在进行，请等其结束。');
    // Institution scope comes from the visible form, not an account/store token.
    if (vm.ruleForm.institutionId !== '1244586319225556993' || modal.ruleForm.institutionId !== vm.ruleForm.institutionId)
      fail('IDENTITY_CONFLICT', '检索所属机构不是上海交通大学。');
    bus = vm.$EventBus;
    if (!bus || typeof bus.$on !== 'function' || typeof bus.$off !== 'function') fail('PAGE_UNSUPPORTED', '查询结果事件接口已变化。');
    const requests = [{kind: 'title', value: command.title.trim(), precise: false}];
    for (const kind of ['doi', 'wos']) {
      const value = command[kind];
      if (value != null && value !== '') {
        if (typeof value !== 'string' || value.length > 300 || /[\x00-\x1f]/.test(value)) fail('IDENTITY_CONFLICT', '标识符必须是完整文本。');
        requests.push({kind, value: value.trim(), precise: true});
      }
    }
    const fields = modal.retrievalFieldList;
    const fieldFor = kind => one(fields.filter(f => typeof f.fieldName === 'string' && f.fieldName &&
      (kind === 'title' ? ['题名', '标题', 'title'].includes(norm(f.label)) && norm(f.fieldName) === 'title'
        : kind === 'doi' ? norm(f.label) === 'doi' && norm(f.fieldName) === 'doi'
        : ['wos', 'wosid', 'wos号', 'wos编号', 'wos唯一号'].includes(norm(f.label)) && ['wos', 'wosid'].includes(norm(f.fieldName)))), `${kind} 检索字段`).fieldName;
    // Resolve every requested field before starting, so partial coverage cannot look like absence.
    requests.forEach(r => r.field = fieldFor(r.kind));
    const allowed = new Set(['aggregationSize', 'beginTime', 'modelIds', 'endTime', 'datasetId', 'datasetIds',
      'containFullText', 'institutionId', 'partition', 'query', 'fieldFilters', 'queryFields', 'issuedYear']);
    if (Object.keys(vm.ruleForm).some(k => !allowed.has(k))) fail('PAGE_UNSUPPORTED', '出现未适配的检索限定，不能据此判断缺失。');
    const queries = [], byId = new Map();
    for (const request of requests) {
      check();
      const form = {aggregationSize: '100', beginTime: null, modelIds: [], endTime: null, datasetId: '',
        containFullText: '', institutionId: vm.ruleForm.institutionId, partition: {category: '', quartile: ''},
        query: {current: 1, size: 10, descs: '_score'}, fieldFilters: [],
        queryFields: [{field: request.field, logicalOperator: null, precise: request.precise, value: request.value},
          {field: 'ALL', logicalOperator: 'AND', precise: false, value: null}]};
      const pages = [], ids = new Set(); let total = null;
      for (let current = 1; ; current++) {
        if (current > 100) fail('RESULT_LIMIT', '结果超过 100 页；请人工缩小查询并核对，不能判定缺失。');
        form.query.current = current;
        const expected = clone(form); let event = null, eventError = null;
        listener = (data, _aggregations, params) => {
          if (!equal(params, expected)) return;
          if (event) { eventError = '同一次查询收到多个结果'; return; }
          try { event = clone(data); } catch { eventError = '结果格式无法保存'; }
        };
        bus.$on('searchItemList', listener);
        try {
          if (vm.loading || modal.loading) fail('PAGE_TIMEOUT', '有其他查询正在运行。');
          if (current === 1) {
            vm.ruleForm = clone(form); modal.ruleForm = clone(form);
            await vm.$nextTick(); check(); modal.submitForm('ruleForm');
          } else {
            vm.ruleForm = clone(form); modal.ruleForm = clone(form);
            await vm.advanceSubmit(clone(form), true);
          }
          await wait(() => eventError || (event && !vm.loading && !modal.loading));
          if (eventError) fail('TASK_CHANGED', eventError);
          if (!equal(vm.ruleForm, expected) || !equal(modal.ruleForm, expected))
            fail('TASK_CHANGED', '查询条件在运行期间变化，结果不能作为本次依据。');
          const page = event?.page;
          if (!page || !Number.isSafeInteger(page.total) || page.total < 0 || !Array.isArray(page.records) ||
              page.current !== current || page.size !== 10)
            fail('PAGE_UNSUPPORTED', '分页总数或结果结构不完整，不能推断零结果。');
          if (total !== null && total !== page.total) fail('TASK_CHANGED', '翻页期间结果总数变化，请重新查询。');
          if (page.records.length !== Math.min(10, Math.max(0, page.total - (current - 1) * 10)))
            fail('PAGE_UNSUPPORTED', '分页条目数不完整，不能推断零结果。');
          if (page.total > 1000) fail('RESULT_LIMIT', '结果超过 100 页，请人工核对，不能判定缺失。');
          total = page.total;
          for (const item of page.records) {
            if (typeof item.id !== 'string' || !item.id || /\s/.test(item.id) || !item.metadata ||
                !Array.isArray(item.metadata.title) || !item.metadata.title.length ||
                !item.metadata.title.every(t => typeof t === 'string' && t.trim()))
              fail('PAGE_UNSUPPORTED', '条目 ID 或完整题名字段缺失。');
            if (ids.has(item.id)) fail('TASK_CHANGED', '不同页出现重复条目，分页结果不完整。');
            ids.add(item.id);
            if (byId.has(item.id) && !equal(byId.get(item.id), item)) fail('TASK_CHANGED', '不同查询中同一条目内容变化。');
            byId.set(item.id, clone(item));
          }
          pages.push(clone(page));
        } finally { bus.$off('searchItemList', listener); listener = null; }
        if (current * 10 >= total) break;
      }
      if (ids.size !== total) fail('PAGE_UNSUPPORTED', '实际读取数与总数不一致。');
      queries.push({...request, total, pages});
    }
    return {ok: true, data: {verified: true, sa_id: command.sa_id, source: location.href,
      institution_id: vm.ruleForm.institutionId, target: {title: command.title.trim(), doi: command.doi || '', wos: command.wos || ''},
      queries, items: [...byId.values()], checked_at: Date.now()}};
  } catch (error) {
    return {ok: false, code: error.code || 'PAGE_UNSUPPORTED', submitted: false, error: error.message};
  } finally { if (bus && listener) bus.$off('searchItemList', listener); }
}
if (typeof module !== 'undefined') module.exports = {runLibraryCommand};
