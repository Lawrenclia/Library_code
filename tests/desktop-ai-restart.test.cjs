// Owned native app/WebView2 processes and an independent loopback API only.
// Uses the production HTTP client, response validator, queue loop and SQLite.
const http = require('node:http'), fs = require('node:fs'), path = require('node:path');
const { spawn } = require('node:child_process'), { randomUUID, createHash } = require('node:crypto');
const { DatabaseSync } = require('node:sqlite'), assert = require('node:assert/strict');
const root = path.resolve(__dirname, '../runtime/tauri-smoke');
const binary = process.env.DESKTOP_TEST_BINARY || path.resolve(__dirname, '../desktop/src-tauri/target/merge-restart/debug/library-workspace.exe');
function snapshot(folder) {
  const db = new DatabaseSync(path.join(folder, 'workspace.sqlite3'), { readOnly: true });
  try {
    db.exec('PRAGMA busy_timeout=5000; BEGIN');
    return { queues: db.prepare('SELECT data FROM ai_queues ORDER BY rowid').all().map(r => JSON.parse(r.data)), tasks: db.prepare('SELECT data FROM tasks ORDER BY rowid').all().map(r => JSON.parse(r.data)), writes: db.prepare('SELECT * FROM attempts').all() };
  } finally { db.close(); }
}
const hash = text => createHash('sha256').update(text).digest('hex');
(async () => {
  let phase = '', boundary, requests = [], savedBoundary;
  const held = new Set(), runs = [], scenarios = [];
  const server = http.createServer((req, res) => {
    let raw = ''; req.setEncoding('utf8'); req.on('data', b => { raw += b; });
    req.on('end', () => {
      try {
        if (req.url === '/restart/ai-saved') {
          assert.equal(phase, 'ai-save'); savedBoundary = JSON.parse(raw);
          held.add(res); res.once('close', () => held.delete(res)); boundary?.(); return;
        }
        assert.equal(req.url, '/api/v1/chat/completions');
        assert.equal(req.method, 'POST'); assert.equal(req.headers.authorization, 'Bearer isolated-fixture-token');
        const body = JSON.parse(raw), input = JSON.parse(body.messages[1].content);
        assert.equal(body.model, 'isolated-ai-fixture'); assert.equal(body.messages[0].role, 'system');
        assert(input.sources.some(e => e.kind === 'roster_input'));
        const number = Number(input.record.title.match(/(\d+)$/)[1]);
        const call = { phase, number, input, body, body_sha256: hash(raw), status: 200 };
        requests.push(call);
        if (phase === 'ai-request' && number === 1) {
          held.add(res); res.once('close', () => held.delete(res)); boundary?.(); return;
        }
        if (phase === 'ai-rate' && number === 1) {
          call.status = 429; res.writeHead(429, { 'Content-Type': 'application/json' }); res.end('{"error":{"message":"isolated rate limit"}}'); return;
        }
        const source = input.sources.find(e => e.kind === 'roster_input');
        const content = { type: null, channel: null, confidence: '低', reason: '隔离响应：待核实成果类型', channel_reason: '需核实数据库收录和导出渠道', evidence_ids: [source.id], missing: ['出版信息与实际类型'], fields: {} };
        const response = { choices: [{ finish_reason: phase === 'ai-ordinary' && number < 4 ? 'length' : 'stop', message: { content: JSON.stringify(content) } }] };
        call.response = response;
        const reply = () => { res.writeHead(200, { 'Content-Type': 'application/json' }); res.end(JSON.stringify(response)); };
        if (phase === 'ai-pause' && number === 1) setTimeout(reply, 800); else reply();
      } catch (error) { res.writeHead(500); res.end(String(error)); }
    });
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${server.address().port}/`;
  async function terminate(child, exited) {
    if (child.exitCode !== null || child.signalCode !== null) return;
    const killer = spawn('taskkill', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true, stdio: 'pipe' });
    const chunks = []; for (const stream of [killer.stdout, killer.stderr]) stream.on('data', b => { chunks.push(b); });
    assert.equal(await new Promise((resolve, reject) => { killer.once('exit', resolve); killer.once('error', reject); }), 0, Buffer.concat(chunks).toString('utf8'));
    await exited;
    const output = Buffer.concat(chunks);
    return { owned_process_id: child.pid, command: ['taskkill', '/PID', String(child.pid), '/T', '/F'], exit_code: 0, output_base64: output.toString('base64'), output_sha256: hash(output) };
  }
  async function run(id, next, crash = false) {
    assert.match(id, /^[0-9a-f-]{36}$/); // Only task-owned UUID directories are used.
    const folder = path.join(root, id); assert.equal(path.dirname(folder), root);
    phase = next; savedBoundary = null;
    const accepted = new Promise(resolve => { boundary = resolve; });
    const env = { ...process.env, DESKTOP_SMOKE_ORIGIN: origin, DESKTOP_SMOKE_RESTART_ID: id, DESKTOP_SMOKE_RESTART_PHASE: next };
    delete env.DESKTOP_SMOKE_LOSE_UPLOAD_RESPONSE;
    const child = spawn(binary, [], { env, windowsHide: true, stdio: 'pipe' });
    let output = ''; for (const stream of [child.stdout, child.stderr]) stream.on('data', b => { output += b; });
    const exited = new Promise((resolve, reject) => { child.once('exit', (code, signal) => resolve({ code, signal })); child.once('error', reject); });
    let timer; const timed = new Promise((_, reject) => { timer = setTimeout(() => reject(Error(`AI ${next} timed out: ${output}`)), 90000); });
    try {
      if (crash) {
        await Promise.race([accepted, exited.then(r => { throw Error(`Exited before AI boundary: ${JSON.stringify(r)} ${output}`); }), timed]);
        const local = snapshot(folder), q = local.queues[0];
        assert.equal(q.status, 'running'); assert.equal(q.cursor, 0); assert(q.inflight);
        assert.deepEqual(q.inflight.input, requests.at(-1).input, 'Durable input must match the actual HTTP request exactly');
        assert.equal(q.targets[0].record.staff_id, '000000000000001');
        if (next === 'ai-save') {
          assert.equal(local.tasks[0].classification.queue_attempt_id, q.inflight.id);
          assert.equal(savedBoundary.process_id, child.pid);
          assert.deepEqual(savedBoundary.queue, q);
          assert(local.tasks[0].evidence.some(e => e.kind === 'ai_classification'));
        } else assert.equal(local.tasks[0].classification, null);
        fs.writeFileSync(path.join(folder, `boundary-${next}.json`), JSON.stringify({ process_id: child.pid, local, requests }, null, 2));
        const termination = await terminate(child, exited);
        assert.equal(fs.existsSync(path.join(folder, `restart-${next}.json`)), false);
        runs.push({ phase: next, process_id: child.pid, terminated: true, termination });
      } else {
        const exit = await Promise.race([exited, timed]);
        const report = JSON.parse(fs.readFileSync(path.join(folder, `restart-${next}.json`), 'utf8'));
        assert.equal(exit.code, 0, `${JSON.stringify(report.failure)} ${output}`); assert.equal(report.passed, true); assert.equal(report.process_id, child.pid);
        assert.equal(report.workspace.running_service, null, 'Completed runner must release the unified execution lock');
        runs.push({ phase: next, process_id: child.pid, passed: true });
      }
      const local = snapshot(folder);
      assert(local.tasks.every(t => t.stage === 'pending' && !t.record.done && t.record.staff_id === '000000000000001'));
      assert.equal(local.writes.length, 0, 'AI must not create platform write intents');
      console.log('AI phase passed:', next, child.pid);
      return folder;
    } finally {
      clearTimeout(timer); await terminate(child, exited); boundary = null;
      for (const reply of held) reply.destroy();
    }
  }
  function completed(folder, firstStatus) {
    const local = snapshot(folder), q = local.queues[0];
    assert.equal(q.status, 'completed'); assert.equal(q.cursor, 4); assert.equal(q.targets.length, 4);
    assert.equal(q.outcomes[0].status, firstStatus);
    assert.deepEqual(q.outcomes.slice(1).map(o => o.status), ['classified', 'classified', 'classified']);
    assert.equal(local.tasks.length, 5, 'A later task should exist without entering the original queue');
    assert.equal(local.tasks.find(t => t.id === 'ai-native-new').classification, null);
    assert.deepEqual(requests.map(r => r.number), [1, 2, 3, 4], 'No original API request may be automatically repeated');
    const source = requests[0].input.sources.find(e => e.id === 'long-original-source');
    assert.equal(source.text, 'Full original source 重要资料🧪。'.repeat(1600));
    assert(fs.existsSync(path.join(folder, 'ai-native-report.xlsx')));
    return local;
  }
  try {
    let id = randomUUID(); requests = [];
    let folder = await run(id, 'ai-ordinary');
    let q = snapshot(folder).queues[0]; assert.equal(q.status, 'completed');
    assert.deepEqual(q.outcomes.map(o => o.status), ['failed', 'failed', 'failed', 'classified']);
    assert.deepEqual(requests.map(r => r.number), [1, 2, 3, 4]);
    scenarios.push({ name: 'ordinary', workspace: folder, requests: structuredClone(requests), queue: q });
    for (const [start, resume, firstStatus, crash] of [
      ['ai-request', 'ai-request-resume', 'unconfirmed', true],
      ['ai-save', 'ai-save-resume', 'classified', true],
      ['ai-pause', 'ai-pause-resume', 'classified', false],
      ['ai-rate', 'ai-rate-resume', 'failed', false]
    ]) {
      id = randomUUID(); requests = []; folder = await run(id, start, crash);
      assert.equal(requests.length, 1, 'Pause/block/crash boundary must prevent later requests');
      const boundaryState = snapshot(folder);
      await run(id, resume);
      const local = completed(folder, firstStatus);
      assert.deepEqual(local.queues[0].outcomes[0].attempt.input, requests[0].input);
      scenarios.push({ name: start, workspace: folder, boundary: boundaryState, restored: local, requests: structuredClone(requests) });
    }
    assert.equal(new Set(runs.map(r => r.process_id)).size, 9);
    const evidence = { passed: true, synthetic: true, real_http_client: true, full_native_process_restart: true, real_provider_verified: false, api_keyring_used: false, runs, scenarios };
    const report = path.join(scenarios[0].workspace, 'ai-restart-acceptance.json');
    fs.writeFileSync(report, JSON.stringify(evidence, null, 2));
    console.log('Native AI restart PASS: nine owned app processes; real local HTTP requests and strict response parsing; ordinary failures continue; durable inputs match actual API bodies; saved result adopted once; unsaved request retained without resend; pause and rate limit preserve remaining scope; original tasks resume without later rows; business stages remain pending. Evidence:', report);
  } finally { for (const reply of held) reply.destroy(); server.close(); server.closeAllConnections(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
