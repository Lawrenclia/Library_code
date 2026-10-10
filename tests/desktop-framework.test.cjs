const { chromium } = require('playwright');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');

(async () => {
  const root = path.resolve(__dirname, '..');
  const registry = spawnSync('cargo', ['run', '--quiet', '--locked', '--manifest-path', 'desktop/core/Cargo.toml', '--example', 'framework_manifest'], { cwd: root, encoding: 'utf8', windowsHide: true });
  assert.equal(registry.status, 0, registry.stderr);
  const manifest = JSON.parse(registry.stdout);
  // Check the actual compiled registry, handler registration and Tauri ACL together.
  const main = fs.readFileSync(path.join(root, 'desktop/src-tauri/src/main.rs'), 'utf8');
  const build = fs.readFileSync(path.join(root, 'desktop/src-tauri/build.rs'), 'utf8');
  const capability = JSON.parse(fs.readFileSync(path.join(root, 'desktop/src-tauri/capabilities/main.json'), 'utf8'));
  const browserCapability = JSON.parse(fs.readFileSync(path.join(root, 'desktop/src-tauri/capabilities/browser.json'), 'utf8'));
  const handlers = [...main.matchAll(/commands::\w+::(\w+)/g)].map(m => m[1]);
  const client = fs.readFileSync(path.join(root, 'desktop/src/services/desktop.ts'), 'utf8');
  const publicCommands = [...client.matchAll(/\|\s*"([a-z_]+)"/g)].map(m => m[1]);
  assert.deepEqual(new Set(publicCommands), new Set(handlers.filter(x => x !== 'browser_result')));
  for (const command of handlers) {
    assert(build.includes('"' + command + '"'), 'Missing build registration: ' + command);
    const permission = 'allow-' + command.replaceAll('_', '-');
    assert((command === 'browser_result' ? browserCapability : capability).permissions.includes(permission), 'Missing ACL: ' + command);
  }
  assert(!browserCapability.permissions.includes('allow-run-step'), 'External pages cannot drive the local workflow.');
  assert.equal(manifest.live_verified, false);

  const browser = await chromium.launch({ executablePath: process.env.TEST_BROWSER || 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1260, height: 860 } });
    await page.addInitScript(registry => {
      window.testCommands = [];
      window.testWorkspace = {
        framework: registry, tasks: [], root: 'isolated-framework-workspace', policy: 'PPT', running: false, paused: false,
        browsers: { wos: { open: true, usable: false }, sa: { open: true, usable: true } },
      };
      window.__TAURI_INTERNALS__ = {
        transformCallback: () => 1, unregisterCallback: () => {},
        invoke: async (command, args) => {
          window.testCommands.push({ command, args });
          if (command === 'workspace') return structuredClone(window.testWorkspace);
          if (command === 'ai_settings') return { base: 'https://example.org/v1', model: 'isolated', configured: false };
          if (command === 'templates') return [];
          if (command === 'plugin:event|listen') return 1;
          if (command === 'import_roster') return { cancelled: true };
          return {};
        },
      };
    }, manifest);
    await page.goto('http://127.0.0.1:1420');
    await page.getByRole('button', { name: '流程与连接', exact: true }).click();
    await page.getByRole('heading', { name: '从名单到处理结果，一个工作空间' }).waitFor();
    assert.equal(await page.getByText('真实平台待验收', { exact: true }).count(), 1);
    assert.equal(await page.getByRole('button', { name: 'WOS 数据库 访问或登录页面', exact: true }).count(), 1);
    assert.equal(await page.getByRole('button', { name: 'SA 比对 工作页已打开', exact: true }).count(), 1);
    for (const flow of manifest.flows) await page.getByRole('heading', { name: flow.label, exact: true }).waitFor();
    await page.getByRole('button', { name: '图书馆数据库入口', exact: true }).click();
    const portal = await page.evaluate(() => window.testCommands.find(c => c.command === 'open_browser'));
    assert.deepEqual(portal.args, { role: 'wos', entry: 'database_directory' });
    assert.equal(await page.evaluate(() => window.testCommands.some(c => c.command === 'run_step')), false, 'Opening the overview cannot submit platform writes.');

    await page.getByRole('tab', { name: '来源与渠道', exact: true }).click();
    for (const channel of manifest.channels) await page.getByRole('heading', { name: channel.label, exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: '打开 WOS 工作页', exact: true }).count(), 1);
    assert.equal(await page.getByRole('button', { name: '管理实际模板', exact: true }).count(), 2);
    const cnki = page.locator('[data-slot="card"]').filter({ has: page.getByRole('heading', { name: 'CNKI 数据导入 (Excel/Txt)', exact: true }) });
    assert.match(await cnki.innerText(), /渠道已登记/);
    assert.equal(await cnki.getByRole('button').count(), 0, 'An unimplemented source has no automatic execution button.');
    await page.getByRole('tab', { name: '功能模块', exact: true }).click();
    for (const service of manifest.services) await page.getByRole('heading', { name: service.label, exact: true }).waitFor();

    // Actual task state controls counters and recovery navigation, not static success text.
    await page.evaluate(() => {
      const record = { matches: 0, done: false, sa_id: 'synthetic', title: 'Synthetic', owner: 'Demo', row: 2, staff_id: '001' };
      window.testWorkspace.tasks = [{ id: 'synthetic', record, route: 'zero_review', stage: 'unknown', revision: 1, running: false, evidence: [], issue_reviews: [], last_error: { code: 'REMOTE_RESULT_UNKNOWN', message: 'Synthetic pending write' }, platform_id: '', review: null, artifact: null, batch: null, classification: null, sa_snapshot: null }];
    });
    await page.getByRole('button', { name: '刷新工作台', exact: true }).click();
    await page.getByRole('button', { name: '1 条结果待确认' }).waitFor();
    await page.getByRole('button', { name: '1 条结果待确认' }).click();
    const reviewFilter = page.locator('.filter-tab').filter({ hasText: '待核验' });
    await reviewFilter.waitFor();
    assert.match(await reviewFilter.getAttribute('class'), /filter-tab-active/);
    await page.getByRole('button', { name: '流程与连接', exact: true }).click();
    await page.getByRole('button', { name: '3. 准备材料' }).click();
    await page.getByText('注册导入模板', { exact: true }).waitFor();
    await page.getByRole('button', { name: '流程与连接', exact: true }).click();
    await page.getByRole('button', { name: '模型与设置', exact: true }).last().click();
    await page.getByText('模型 API', { exact: true }).waitFor();
    await page.getByRole('button', { name: '流程与连接', exact: true }).click();
    await page.getByRole('tab', { name: '处理分支', exact: true }).click();
    for (const viewport of [{ width: 1260, height: 860 }, { width: 960, height: 680 }]) {
      await page.setViewportSize(viewport);
      assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'Framework overflows at ' + viewport.width);
      const folder = path.join(root, 'runtime/framework-preview'); fs.mkdirSync(folder, { recursive: true });
      await page.screenshot({ path: path.join(folder, `framework-${viewport.width}.png`), fullPage: true });
    }
    console.log('Framework: compiled service/source registry, public IPC/ACL consistency, live task counts, recovery routing, real entry actions, unsupported channel disclosure, 1260/960 layout passed. Isolated UI only; no platform writes.');
  } finally { await browser.close(); }
})().catch(e => { console.error(e); process.exit(1); });
