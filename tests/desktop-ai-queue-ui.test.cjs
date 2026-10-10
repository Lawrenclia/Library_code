// Real Vue UI, isolated IPC and model replies; no user profile or API request.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path');
(async () => {
  const browser = await chromium.launch({ executablePath: process.env.TEST_BROWSER || 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1260, height: 860 } });
    await page.addInitScript(() => {
      const task = (id, owner) => ({ id, paper_id: '', revision: 0, record: { row: 2, owner, sa_id: id, title: '批量分类测试论文', doi: '', wos: '', staff_id: '001', matches: 0, item_ids: '', mark: '待处理', reason: '原文保留', skipped: false, done: false, source: '' }, route: 'zero_review', stage: 'pending', running: false, evidence: [], last_error: null, review: null, classification: null, artifact: null, platform_id: '', batch: null, sa_snapshot: null });
      window.work = { tasks: [task('one', '原负责人'), task('two', '其他负责人')], root: 'isolated-ai-queue', running: false, running_service: null, paused: false, browsers: {}, policy: 'PPT' };
      window.commands = []; window.callbacks = {}; window.callbackId = 0;
      window.emitChange = () => { if (window.eventHandler) callbacks[eventHandler]({ event: 'workspace-changed', id: 1, payload: {} }); };
      window.__TAURI_INTERNALS__ = {
        transformCallback: fn => { callbacks[++callbackId] = fn; return callbackId; }, unregisterCallback: () => {},
        invoke: async (command, args) => {
          commands.push({ command, args });
          if (command === 'workspace') return structuredClone(work);
          if (command === 'ai_settings') return { base: 'https://example.test/v1', model: 'isolated', configured: true };
          if (command === 'templates') return [{ id: 'template-one', name: '原测试模板', columns: ['Title'], required: ['Title'], notes: '' }];
          if (command === 'plugin:event|listen') { if (args.event === 'workspace-changed') window.eventHandler = args.handler; return 1; }
          if (command === 'run_ai_queue') {
            work.ai_queue = { id: 'original-ai-queue', owner: args.owner, zero_only: args.zeroOnly, config: { base: 'https://example.test/v1', model: 'isolated' }, template: { id: args.templateId, name: '原测试模板' }, status: 'running', cursor: 0, targets: [{ id: 'one' }, { id: 'original-second' }, { id: 'original-third' }], outcomes: [], pause_requested: false, inflight: { task_id: 'one' }, last_error: null };
            work.running = true; work.running_service = 'ai'; emitChange();
            await new Promise(resolve => { window.releaseRun = resolve; });
            return {};
          }
          if (command === 'pause_ai_queue' || command === 'pause_queue') {
            work.ai_queue.status = 'paused'; work.ai_queue.pause_requested = true; work.ai_queue.inflight = null;
            work.ai_queue.cursor = 1; work.ai_queue.outcomes = [{ id: 'one', status: 'unconfirmed', error: { message: '请求结果未保存，未重发' } }];
            work.running = false; work.running_service = null; work.paused = true;
            if (window.releaseRun) { releaseRun(); window.releaseRun = null; }
            emitChange(); return {};
          }
          if (command === 'resume_ai_queue') { work.ai_queue.status = 'blocked'; work.ai_queue.last_error = { code: 'AI_RATE_LIMIT', message: 'API 限流，保留原范围剩余项' }; return {}; }
          if (command === 'cancel_ai_queue') { work.ai_queue.status = 'cancelled'; return {}; }
          return {};
        }
      };
    });
    await page.goto('http://127.0.0.1:1420');
    const panel = page.getByRole('region', { name: 'AI 批量处理', exact: true });
    await panel.getByRole('button', { name: '展开批量 AI 配置', exact: true }).click();
    await page.getByLabel('负责人', { exact: true }).selectOption('原负责人');
    await panel.getByLabel('AI 批量范围').selectOption({ label: '当前负责人全部待处理论文' });
    await panel.getByLabel('AI 批量模板').selectOption('template-one');
    await panel.getByRole('button', { name: '开始批量 AI', exact: true }).click();
    await panel.getByRole('button', { name: '暂停 AI 后续任务', exact: true }).waitFor();
    assert.deepEqual(await page.evaluate(() => commands.find(c => c.command === 'run_ai_queue').args), { owner: '原负责人', zeroOnly: false, templateId: 'template-one' });
    // The batch invocation is still pending. Pause must bypass its pending lock.
    await panel.getByRole('button', { name: '暂停 AI 后续任务', exact: true }).click();
    await panel.getByText(/已暂停 · 原范围/).waitFor();
    assert(await page.evaluate(() => commands.some(c => c.command === 'pause_ai_queue')));
    await page.getByLabel('负责人', { exact: true }).selectOption('其他负责人');
    assert.match(await panel.innerText(), /原负责人/);
    assert.match(await panel.innerText(), /全部待处理/);
    assert.match(await panel.innerText(), /原测试模板/);
    assert.match(await panel.innerText(), /已记录 1 \/ 3 篇/);
    assert.equal(await panel.getByLabel('AI 批量模板').count(), 0, 'Frozen configuration replaces editable scope');
    await panel.locator('summary').click();
    assert.match(await panel.innerText(), /请求结果待核对，未重发/);
    await panel.getByRole('button', { name: '核对并继续原 AI 范围', exact: true }).click();
    await panel.getByText('API 限流，保留原范围剩余项', { exact: true }).waitFor();
    assert.deepEqual(await page.evaluate(() => commands.find(c => c.command === 'resume_ai_queue').args), { id: 'original-ai-queue' });
    // A stale stored AI status must not display an AI pause for another running service.
    await page.evaluate(() => { work.ai_queue.status = 'running'; work.running = true; work.running_service = 'download'; emitChange(); });
    await page.waitForFunction(() => document.querySelector('[aria-label="AI 批量处理"]').textContent.includes('处理中'));
    assert.equal(await panel.getByRole('button', { name: '暂停 AI 后续任务', exact: true }).count(), 0);
    assert.equal(await panel.getByRole('button', { name: '核对并继续原 AI 范围', exact: true }).isDisabled(), true);
    await page.getByRole('button', { name: '暂停后续任务', exact: true }).click();
    await panel.getByText(/已暂停 · 原范围/).waitFor();
    assert(await page.evaluate(() => commands.some(c => c.command === 'pause_queue')));
    for (const width of [1260, 960]) {
      await page.setViewportSize({ width, height: 760 });
      assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      const dir = path.resolve(__dirname, '../runtime/ai-queue-preview'); fs.mkdirSync(dir, { recursive: true });
      await page.screenshot({ path: path.join(dir, `ai-queue-${width}.png`), fullPage: true });
    }
    await panel.getByRole('button', { name: '结束原 AI 范围', exact: true }).click();
    await panel.getByText(/原范围已结束/).waitFor();
    assert.deepEqual(await page.evaluate(() => commands.find(c => c.command === 'cancel_ai_queue').args), { id: 'original-ai-queue' });
    assert.match(await panel.innerText(), /请求结果待核对，未重发/);
    assert.equal(await page.evaluate(() => commands.some(c => ['run_step', 'fill_template', 'classify_task'].includes(c.command))), false);
    assert.equal(await page.evaluate(() => work.tasks[0].stage), 'pending');
    console.log('AI queue UI passed: exact start scope/template, pause during pending request, frozen owner/scope, original resume ID, rate-limit retention, interrupted-request disclosure, service-aware pause, cancel history, 1260/960 layout. Isolated IPC; no API charge or platform write.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
