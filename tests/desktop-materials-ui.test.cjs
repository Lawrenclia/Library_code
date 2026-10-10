const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
(async () => {
  const browser = await chromium.launch({ executablePath: process.env.TEST_BROWSER || 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1260, height: 860 } });
    await page.addInitScript(() => {
      const record = { row: 2, owner: '材料测试', sa_id: '10001', title: '材料来源论文', doi: '', wos: '', staff_id: '001', matches: 0, item_ids: '', mark: '待处理', reason: '', skipped: false, done: false, source: '' };
      const classification = { type: '期刊论文', channel: 'general', reason: '隔离测试', evidence_ids: ['source'], missing: [], template_id: 'template-1', fields: { '发表日期': { value: '2023-02-29', evidence_ids: ['source'] } } };
      const task = { id: '10001', paper_id: '', revision: 0, record, route: 'zero_review', stage: 'pending', running: false, evidence: [{ id: 'source', kind: 'metadata', source: '隔离测试', text: '论文来源', created: 0 }], last_error: null, review: null, classification, artifact: null, platform_id: '', batch: null, sa_snapshot: null };
      window.testWorkspace = { tasks: [task, { ...task, id: '10002', record: { ...record, sa_id: '10002', title: '另一条材料任务' } }], root: 'isolated-materials', running: false, paused: false, browsers: {}, policy: 'PPT' };
      window.materialReport = { path: 'isolated/draft.xlsx', missing: ['题名'], invalid: [{ column: '发表日期', reason: '实际日期无效', rule_source: 'Sheet1!G1：yyyy-MM-dd' }], requires_review: [{ column: '代码', reason: '需人工核对', rule_source: 'Sheet1!AA3 动态枚举' }], ready: false };
      window.testCommands = [];
      window.__TAURI_INTERNALS__ = { transformCallback: () => 1, unregisterCallback: () => {}, invoke: async (command, args) => {
        window.testCommands.push({ command, args });
        if (command === 'workspace') return structuredClone(window.testWorkspace);
        if (command === 'ai_settings') return { base: 'https://example.org/v1', model: 'isolated', configured: true };
        if (command === 'templates') return [{ id: 'template-1', name: '模板一', path: 'isolated/template.xlsx', sheet: 'Sheet1', header_row: 1, columns: ['题名', '发表日期', '页数 [A列]', '页数 [B列]'], headers: ['题名', '发表日期', '页数', '页数'], required: ['题名'], notes: '原表说明', field_rules: [{ column: '发表日期', source: 'Sheet1!G1', check: { kind: 'publication_date' } }] }, { id: 'template-2', name: '模板二', columns: [], required: [], notes: '' }];
        if (command === 'plugin:event|listen') return 1;
        if (command === 'fill_template') {
          const completed = () => { const task = window.testWorkspace.tasks.find(t => t.id === args.id); task.revision++; return { ...structuredClone(window.materialReport), task_revision: task.revision }; };
          if (window.deferMaterial) return new Promise(resolve => { window.releaseMaterial = () => resolve(completed()); });
          return completed();
        }
        return {};
      } };
    });
    await page.goto('http://127.0.0.1:1420');
    await page.getByText('材料来源论文', { exact: true }).first().click();
    await page.getByRole('tab', { name: 'AI 建议', exact: true }).click();
    await page.getByLabel('填写模板', { exact: true }).selectOption('template-1');
    const fill = page.getByRole('button', { name: '检查必填项并导出模板材料', exact: true });
    await fill.click();
    const report = page.getByTestId('material-validation');
    await report.getByText('材料草稿待补充或核对', { exact: true }).waitFor();
    assert((await report.textContent()).includes('必填缺项：题名'));
    assert((await report.textContent()).includes('实际日期无效'));
    assert((await report.textContent()).includes('动态枚举'));
    assert((await report.textContent()).includes('未通过校验的字段已留空'));
    assert.equal(await page.evaluate(() => window.testCommands.some(c => c.command === 'run_step')), false);
    await page.getByLabel('填写模板', { exact: true }).selectOption('template-2');
    assert.equal(await report.count(), 0, 'Changing the template clears its old validation.');
    await page.getByLabel('填写模板', { exact: true }).selectOption('template-1');
    await page.evaluate(() => { window.materialReport = { path: 'isolated/valid.xlsx', missing: [], invalid: [], requires_review: [], ready: true }; });
    await fill.click(); await report.getByText('模板校验通过', { exact: true }).waitFor();
    fs.mkdirSync(path.resolve(__dirname, '../runtime/materials-preview'), { recursive: true });
    await page.screenshot({ path: path.resolve(__dirname, '../runtime/materials-preview/materials-1260.png') });
    await page.setViewportSize({ width: 960, height: 680 });
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    await page.evaluate(() => { window.deferMaterial = true; });
    await fill.click();
    await page.waitForFunction(() => typeof window.releaseMaterial === 'function');
    await page.getByLabel('填写模板', { exact: true }).selectOption('template-2');
    await page.evaluate(() => window.releaseMaterial());
    await page.waitForFunction(() => !document.querySelector('button[disabled]')?.textContent?.includes('检查必填'));
    assert.equal(await report.count(), 0, 'A late export response cannot populate another template.');
    await page.getByRole('button', { name: '模板与材料', exact: true }).click();
    await page.getByText('查看可填写列名', { exact: true }).first().click();
    await page.getByText('题名、发表日期、页数 [A列]、页数 [B列]', { exact: true }).waitFor();
    console.log('Materials UI: incomplete drafts, rule sources, omitted invalid values, valid result, template switching, late response, duplicate column keys and 1260/960 layout passed. Isolated IPC; no real API or platform writes.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
