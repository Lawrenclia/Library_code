const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
(async () => {
  const browser = await chromium.launch({ executablePath: process.env.TEST_BROWSER || 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1260, height: 860 } });
    await page.addInitScript(() => {
      const record = { row: 2, owner: '来源测试', sa_id: '10001', title: '来源测试论文', doi: '10.1234/test', wos: '', staff_id: '001', matches: 0, item_ids: '', mark: '待处理', reason: '', skipped: false, done: false, source: '' };
      const task = { id: '10001', paper_id: '', revision: 0, record, route: 'zero_review', stage: 'pending', running: false, evidence: [], last_error: null, review: null, classification: null, artifact: null, platform_id: '', batch: null, sa_snapshot: null };
      window.testWorkspace = { tasks: [task, { ...task, id: '10002', record: { ...record, sa_id: '10002', title: '另一条来源任务' } }], root: 'isolated-sources', running: false, paused: false, browsers: {}, policy: 'PPT', framework: { channels: [{ id: 'cnki', label: 'CNKI 数据导入 (Excel/Txt)', formats: ['xlsx', 'txt'] }, { id: 'ei', label: 'EI 数据导入 (Csv/Excel)', formats: ['csv', 'xlsx'] }, { id: 'other', label: '其他来源', formats: ['xlsx', 'txt', 'csv'] }] } };
      window.testCommands = []; window.sourceMode = 'table';
      window.makePreview = (args) => {
        const t = window.testWorkspace.tasks.find(t => t.id === args.id);
        const text = window.sourceMode === 'txt';
        return { draft: { id: 'draft-' + args.id, task_id: args.id, task_revision: t.revision, channel: args.channel || 'ei', original_name: text ? '原始.txt' : '原始.csv', path: 'isolated/source-files/hash.' + (text ? 'txt' : 'csv'), sha256: 'a'.repeat(64), format: text ? 'txt' : 'csv' }, actual_encoding: text ? 'GB18030' : 'UTF-8', sheet: text ? '文本' : args.sheet || 'CSV', sheets: text ? [{ name: '文本', first_row: 1, rows: 4 }] : [{ name: 'CSV', first_row: 1, rows: 62 }, { name: 'Second', first_row: 1, rows: 62 }], header_row: args.headerRow || 1, page: args.page || 0, total: text ? 4 : 61,
          columns: text ? [] : [{ column: 0, name: 'Title', label: 'Title [A列]' }, { column: 1, name: 'Name', label: 'Name [B列]' }, { column: 2, name: 'Name', label: 'Name [C列]' }, { column: 3, name: 'DOI', label: 'DOI [D列]' }, { column: 4, name: '', label: ' [E列]' }],
          rows: text ? [{ row: 1, values: ['Other'] }, { row: 2, values: ['Source title'] }, { row: 3, values: ['Full abstract'] }, { row: 4, values: ['Other end'] }] : args.page ? [{ row: 52, values: ['Page two paper', 'Li', 'Wang', '10.1234/test', 'full abstract'] }] : [{ row: 2, values: ['Wrong first record', 'Other', 'Other', '', ''] }, { row: 3, values: ['Source title', 'Li', 'Wang', '10.1234/test', '完整摘要；不能遗漏。'] }] };
      };
      window.__TAURI_INTERNALS__ = { transformCallback: () => 1, unregisterCallback: () => {}, invoke: async (command, args) => {
        window.testCommands.push({ command, args });
        if (command === 'workspace') return structuredClone(window.testWorkspace);
        if (command === 'ai_settings') return { base: 'https://example.org/v1', model: 'isolated', configured: false };
        if (command === 'templates') return [];
        if (command === 'plugin:event|listen') return 1;
        if (command === 'preview_source_file') {
          const response = window.makePreview(args);
          if (window.deferSource) return new Promise(resolve => { window.releaseSource = () => resolve(response); });
          return response;
        }
        if (command === 'source_file_page') return window.makePreview(args);
        if (command === 'attach_source_file') {
          const t = window.testWorkspace.tasks.find(t => t.id === args.id);
          t.revision++; t.classification = null;
          t.evidence.push({ id: 'attached', kind: 'external_metadata', source: args.sourceUrl, text: JSON.stringify({ schema: 'original_source_v1', title: 'Source title', channel: 'ei', original_name: '原始.csv', archive_path: 'isolated/source-files/hash.csv', sha256: 'a'.repeat(64), selection: args.selection, binding_note: args.note, institution_verified: false }), created: 0 });
          return { task_revision: t.revision, stage: t.stage, attached: true };
        }
        return {};
      } };
    });
    await page.goto('http://127.0.0.1:1420');
    await page.getByText('来源测试论文', { exact: true }).first().click();
    await page.getByRole('tab', { name: '来源', exact: true }).click();
    const panel = page.getByTestId('source-importer');
    const choose = panel.getByRole('button', { name: '选择原始来源文件', exact: true });
    const attach = panel.getByRole('button', { name: '绑定本条原始来源', exact: true });
    await panel.getByLabel('原始来源渠道').selectOption('ei');
    await panel.getByLabel('来源 CSV 分隔符').selectOption('semicolon');
    await choose.click();
    await panel.getByTestId('source-preview').waitFor();
    assert(await attach.isDisabled());
    assert.equal(await panel.locator('input[type=radio]:checked').count(), 0);
    assert.match(await panel.innerText(), /本地来源读取不表示数据库自动导出/);
    const call = await page.evaluate(() => window.testCommands.find(c => c.command === 'preview_source_file'));
    assert.deepEqual(call.args, { id: '10001', channel: 'ei', options: { encoding: 'utf-8', delimiter: 'semicolon' } });
    await panel.getByLabel('选择来源记录').nth(1).check();
    await panel.getByLabel('来源题名列').selectOption('0');
    await panel.getByLabel('来源 DOI 列').selectOption('3');
    await panel.getByText('查看所选记录的全部字段', { exact: true }).click();
    await panel.getByText('完整摘要；不能遗漏。', { exact: true }).waitFor();
    await panel.getByText('Name [B列]', { exact: true }).last().waitFor();
    await panel.getByText('Name [C列]', { exact: true }).last().waitFor();
    await panel.getByLabel('来源绑定依据').fill('Checked identifiers and source title');
    await panel.getByLabel('原始来源记录链接').fill('https://example.org/record');
    assert(await attach.isDisabled());
    await panel.getByRole('checkbox').check();
    assert(await attach.isEnabled());
    await panel.getByLabel('来源表头行号').fill('2');
    assert(await attach.isDisabled(), 'Unrefreshed header cannot attach cached rows');
    await panel.getByRole('button', { name: '按所选工作表和表头读取', exact: true }).click();
    assert.equal(await panel.locator('input[type=radio]:checked').count(), 0);
    await panel.getByLabel('来源工作表').selectOption('Second');
    await panel.getByRole('button', { name: '按所选工作表和表头读取', exact: true }).click();
    const refreshed = await page.evaluate(() => window.testCommands.filter(c => c.command === 'source_file_page').at(-1));
    assert.deepEqual(refreshed.args, { id: '10001', previewId: 'draft-10001', sheet: 'Second', headerRow: 2, page: 0 });
    await panel.getByRole('button', { name: '下一页', exact: true }).click();
    await panel.getByLabel('选择来源记录').check();
    await panel.getByLabel('来源题名列').selectOption('0');
    await panel.getByLabel('来源 DOI 列').selectOption('3');
    await panel.getByRole('checkbox').check();
    await attach.click();
    await panel.getByText('原始来源已绑定；文献身份、归属与平台处理分别核验。历史来源保留在记录中。', { exact: true }).waitFor();
    const bound = await page.evaluate(() => window.testCommands.find(c => c.command === 'attach_source_file'));
    assert.deepEqual(bound.args.selection, { sheet: 'Second', header_row: 2, row: 52, end_row: 52, title_column: 0, doi_column: 3, wos_column: null, text_title: '' });
    assert.equal(bound.args.confirmed, true);
    assert.equal(await page.evaluate(() => window.testWorkspace.tasks[0].stage), 'pending');
    assert.equal(await page.evaluate(() => window.testWorkspace.tasks[0].artifact), null);
    assert.equal(await panel.getByTestId('source-preview').count(), 0, 'Committed revision clears old preview');
    await panel.getByRole('button', { name: '继续上次来源预览', exact: true }).click();
    const resumed = await page.evaluate(() => window.testCommands.filter(c => c.command === 'source_file_page').at(-1));
    assert.equal(resumed.args.previewId, null); assert.equal(resumed.args.headerRow, 0);
    await page.evaluate(() => { window.sourceMode = 'txt'; });
    await panel.getByLabel('原始来源渠道').selectOption('other');
    await panel.getByLabel('来源文本编码').selectOption('gb18030');
    await choose.click();
    await panel.getByLabel('来源文本开始行').fill('2');
    await panel.getByLabel('来源文本结束行').fill('3');
    await panel.getByLabel('来源文本原文题名').fill('Source title');
    await panel.getByLabel('来源绑定依据').fill('Checked the complete selected text');
    await panel.getByRole('checkbox').check();
    await attach.click();
    const txt = await page.evaluate(() => window.testCommands.filter(c => c.command === 'attach_source_file').at(-1));
    assert.deepEqual(txt.args.selection, { sheet: '文本', header_row: 1, row: 2, end_row: 3, title_column: null, doi_column: null, wos_column: null, text_title: 'Source title' });
    assert.equal(await page.evaluate(() => window.testCommands.some(c => ['run_step', 'adopt_file', 'classify_task'].includes(c.command))), false);
    fs.mkdirSync(path.resolve(__dirname, '../runtime/sources-preview'), { recursive: true });
    await page.screenshot({ path: path.resolve(__dirname, '../runtime/sources-preview/sources-1260.png') });
    await page.setViewportSize({ width: 960, height: 680 });
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    await page.evaluate(() => { window.deferSource = true; });
    await choose.click();
    await page.waitForFunction(() => typeof window.releaseSource === 'function');
    await page.getByText('另一条来源任务', { exact: true }).first().click();
    await page.evaluate(() => window.releaseSource());
    await page.waitForFunction(() => window.testCommands.filter(c => c.command === 'workspace').length > 5);
    assert.equal(await page.getByTestId('source-preview').count(), 0, 'Late preview cannot populate another task');
    console.log('Sources UI passed: explicit row/range/columns, full fields, pagination, header/sheet refresh, confirmation, receipt, revision reset, resume IPC, task isolation and 1260/960 layout. Isolated IPC only.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
