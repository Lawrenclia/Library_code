// Actual app/WebView2 processes; independent HTTP state survives every crash.
// All business pages are local synthetic fixtures, not the user's live session.
const http = require('node:http'), fs = require('node:fs'), path = require('node:path');
const { spawn } = require('node:child_process'), { randomUUID } = require('node:crypto');
const { DatabaseSync } = require('node:sqlite'), assert = require('node:assert/strict');
const fixture = name => fs.readFileSync(path.join(__dirname, 'fixtures', name), 'utf8');
const root = path.resolve(__dirname, '../runtime/tauri-smoke');
const binary = process.env.DESKTOP_TEST_BINARY || path.resolve(__dirname, '../desktop/src-tauri/target/merge-restart/debug/library-workspace.exe');
const initial = () => {
  const primary = { id: 'item-primary', modelName: '期刊论文', metadata: { title: ['Synthetic paper'], doi: ['10.1234/test'], author: [{ fullname: 'Demo, A' }], year: ['2026'] } };
  const source = { id: 'item-source', modelName: '科技论文', metadata: { ...structuredClone(primary.metadata), title: ['Synthetic paper corrected'], abstract: ['Full original source abstract. 重要内容不省略。'] } };
  const third = { id: 'item-third', modelName: '期刊论文', metadata: { ...structuredClone(primary.metadata), keywords: ['original third keyword'] } };
  return { groups: [{ id: 'group-1', item: primary, items: [{ item: source }, { item: third }] }], masters: Object.fromEntries([primary, source, third].map(v => [v.id, structuredClone(v)])), ids: ['item-primary', 'item-source', 'item-third'], writes: [], reads: 0 };
};
function snapshot(folder) {
  const db = new DatabaseSync(path.join(folder, 'workspace.sqlite3'), { readOnly: true });
  try { db.exec('PRAGMA busy_timeout=5000'); return { tasks: db.prepare('SELECT data FROM tasks').all().map(r => JSON.parse(r.data)), attempts: db.prepare('SELECT * FROM attempts ORDER BY created,rowid').all().map(r => ({ ...r, data: JSON.parse(r.data) })) }; }
  finally { db.close(); }
}
(async () => {
  let remote = initial(), phase = '', boundary, absent = false;
  const replies = new Set(), runs = [];
  const duplicate = () => fixture('duplicate.html').replaceAll('A paper', 'Synthetic paper').replace('</script>', `
    async function remoteState(){const state=await(await fetch('/restart/merge-state')).json();window.testGroups=state.groups;window.testMasters=state.masters;return state;}
    const originalQuery=vm.getData.bind(vm);vm.getData=async function(query){await remoteState();return originalQuery(query);};
    const originalList=modal.getList.bind(modal);modal.getList=function(){this.list=[];render();remoteState().then(()=>originalList());};
    const originalDetail=detailInner.getItemDetail.bind(detailInner);detailInner.getItemDetail=async function(){await remoteState();return originalDetail();};
    const originalEmit=modal.$emit.bind(modal);modal.$emit=function(event){
      if(event==='upData'){const payload={group_id:this.id,...clone(this.merge)};fetch('/restart/merge-save',{method:'POST',body:JSON.stringify(payload)}).then(response=>{if(response.ok)originalEmit(event);});}
      else originalEmit(event);
    };
    vm.page.titleSimilarity=${JSON.stringify(phase === 'merge-threshold-reject' ? '94.50' : '93.50')};
  </script>`);
  const sa = () => fixture('compare.html').replace('</script>', `
    const originalQuery=vm.getData.bind(vm);vm.getData=async function(){
      const state=await(await fetch('/restart/merge-state')).json();
      const ids=${phase === 'merge-ids-reject' ? "['item-primary']" : 'state.ids'};
      Object.assign(synthetic,{saLzkId:'restart-merge',gh:'001',itemId:ids.join(','),matchCount:ids.length,reason:'重复数据',markStatus:'待处理',remark:${JSON.stringify(phase === 'merge-sa-reject' ? 'Changed unrelated SA remark' : '')}});
      testConfig.saDoi=${JSON.stringify(phase === 'merge-comparison-reject' ? '10.1234/changed-comparison' : '10.1234/test')};
      return originalQuery();
    };
  </script>`);
  const server = http.createServer((req, res) => {
    if (req.url === '/restart/merge-state') {
      remote.reads++; const state = structuredClone(remote);
      if (phase === 'merge-master-reject') state.masters['item-primary'].metadata.year = ['1999'];
      if (phase === 'merge-pool-reject') state.groups.push({ id: 'unrelated-group', item: initial().masters['item-source'], items: [{ item: initial().masters['item-third'] }] });
      res.writeHead(200, { 'Content-Type': 'application/json' }); res.end(JSON.stringify(state)); return;
    }
    if (req.url === '/restart/merge-save' && req.method === 'POST') {
      let body = ''; req.on('data', b => body += b); req.on('end', () => {
        try {
          const p = JSON.parse(body); assert.equal(p.group_id, 'group-1'); assert.equal(p.targetItemId, 'item-primary');
          assert.equal(p.itemId, phase === 'merge-next-start' ? 'item-third' : 'item-source');
          remote.writes.push(structuredClone(p));
          if (!absent) {
            const original = remote.masters[p.itemId], master = remote.masters[p.targetItemId];
            for (const [k, v] of Object.entries(original.metadata)) if (!Object.hasOwn(master.metadata, k)) master.metadata[k] = structuredClone(v);
            remote.ids = remote.ids.filter(id => id !== p.itemId); delete remote.masters[p.itemId];
            remote.groups[0].item = structuredClone(master);
            remote.groups[0].items = remote.groups[0].items.filter(v => v.item.id !== p.itemId);
            if (!remote.groups[0].items.length) remote.groups = [];
          }
          replies.add(res); res.once('close', () => replies.delete(res)); boundary?.();
          // Keep the business success response pending until the app is killed.
        } catch (error) { res.writeHead(500); res.end(String(error)); }
      }); return;
    }
    const html = req.url === '/native/duplicate' ? duplicate() : sa();
    for (const m of html.matchAll(/<script>([\s\S]*?)<\/script>/g)) new (require('node:vm').Script)(m[1]);
    res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' }); res.end(html);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve)); const origin = `http://127.0.0.1:${server.address().port}/`;
  async function terminate(child, exited) {
    if (child.exitCode !== null || child.signalCode !== null) return;
    const killer = spawn('taskkill', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true, stdio: 'pipe' }); let output = '';
    killer.stderr.on('data', b => output += b);
    assert.equal(await new Promise((resolve, reject) => { killer.once('exit', resolve); killer.once('error', reject); }), 0, output);
    await exited;
  }
  async function run(id, next, crash = false) {
    phase = next; const accepted = new Promise(resolve => boundary = resolve), folder = path.join(root, id);
    const env = { ...process.env, DESKTOP_SMOKE_ORIGIN: origin, DESKTOP_SMOKE_RESTART_ID: id, DESKTOP_SMOKE_RESTART_PHASE: next };
    delete env.DESKTOP_SMOKE_LOSE_UPLOAD_RESPONSE;
    const child = spawn(binary, [], { env, windowsHide: true, stdio: 'pipe' }); let output = '';
    const exited = new Promise((resolve, reject) => { child.once('exit', (code, signal) => resolve({ code, signal })); child.once('error', reject); });
    for (const stream of [child.stdout, child.stderr]) stream.on('data', b => { output += b; process.stdout.write(b); });
    let timer; const timed = new Promise((_, reject) => timer = setTimeout(() => reject(Error(`Merge ${next} timed out: ${output}`)), 150000));
    try {
      if (crash) {
        await Promise.race([accepted, exited.then(r => { throw Error(`Exited before accepted merge: ${JSON.stringify(r)} ${output}`); }), timed]);
        const local = snapshot(folder), active = local.attempts.filter(a => a.state === 'intent');
        assert.equal(local.tasks.length, 1); assert.equal(local.tasks[0].running, true); assert.equal(active.length, 1);
        assert.equal(active[0].action, 'merge_duplicate'); assert.equal(active[0].data.payload.schema, 'merge_context_v1');
        assert.equal(remote.writes.length, next === 'merge-next-start' ? 2 : 1);
        fs.writeFileSync(path.join(folder, `boundary-${next}.json`), JSON.stringify({ process_id: child.pid, local, remote }, null, 2));
        await terminate(child, exited); assert.equal(fs.existsSync(path.join(folder, `restart-${next}.json`)), false);
        runs.push({ phase: next, process_id: child.pid, terminated: true });
      } else {
        const exit = await Promise.race([exited, timed]); const report = JSON.parse(fs.readFileSync(path.join(folder, `restart-${next}.json`), 'utf8'));
        assert.equal(exit.code, 0, JSON.stringify(report.failure)); assert.equal(report.passed, true, JSON.stringify(report.failure)); assert.equal(report.process_id, child.pid);
        runs.push({ phase: next, process_id: child.pid, passed: true });
      }
      console.log('Merge phase passed:', next, child.pid); return folder;
    } finally { clearTimeout(timer); await terminate(child, exited); boundary = null; for (const reply of replies) reply.destroy(); }
  }
  function unknown(folder, original) {
    const s = snapshot(folder); assert.equal(s.tasks[0].stage, 'unknown'); assert.equal(s.tasks[0].record.done, false);
    assert.equal(s.tasks[0].merges.length, 0); assert.equal(s.attempts.length, 1); assert.equal(s.attempts[0].state, 'unknown');
    assert.deepEqual(s.attempts[0].data.payload, original.local.attempts[0].data.payload);
    assert.equal(s.tasks[0].evidence.filter(e => e.kind === 'duplicate_merge_verified').length, 0); assert.equal(remote.writes.length, 1);
    return s;
  }
  try {
    const id = randomUUID(), folder = await run(id, 'merge-start', true);
    const original = JSON.parse(fs.readFileSync(path.join(folder, 'boundary-merge-start.json'), 'utf8'));
    for (const reject of ['merge-master-reject', 'merge-pool-reject', 'merge-sa-reject', 'merge-comparison-reject', 'merge-ids-reject', 'merge-threshold-reject']) {
      await run(id, reject); unknown(folder, original);
    }
    await run(id, 'merge-resume'); const first = snapshot(folder);
    assert.equal(first.tasks[0].stage, 'pending'); assert.equal(first.tasks[0].merges.length, 1); assert.equal(first.tasks[0].record.matches, 3);
    assert.equal(first.attempts[0].state, 'verified'); assert.deepEqual(first.attempts[0].data.payload, original.local.attempts[0].data.payload);
    assert.deepEqual(remote.ids, ['item-primary', 'item-third']); assert.equal(remote.writes.length, 1);
    assert.equal(remote.masters['item-primary'].metadata.abstract[0], initial().masters['item-source'].metadata.abstract[0]);
    await run(id, 'merge-next-start', true);
    const secondOriginal = JSON.parse(fs.readFileSync(path.join(folder, 'boundary-merge-next-start.json'), 'utf8'));
    await run(id, 'merge-next-resume'); const final = snapshot(folder);
    assert.equal(final.tasks[0].stage, 'pending'); assert.equal(final.tasks[0].record.done, false); assert.equal(final.tasks[0].record.matches, 3);
    assert.equal(final.tasks[0].platform_id, 'item-primary'); assert.equal(final.tasks[0].merges.length, 2);
    assert.equal(final.attempts.length, 2); assert(final.attempts.every(a => a.state === 'verified'));
    assert.deepEqual(final.attempts[0].data.payload, original.local.attempts[0].data.payload);
    assert.deepEqual(final.attempts[1].data.payload, secondOriginal.local.attempts[1].data.payload);
    assert.deepEqual(final.attempts[1].data.payload.previous_merges, first.tasks[0].merges);
    const proofs = final.tasks[0].evidence.filter(e => e.kind === 'duplicate_merge_verified').map(e => JSON.parse(e.text));
    assert.equal(proofs.length, 2); assert.deepEqual(proofs.map(p => p.sa_after.row.matchCount), [2, 1]);
    assert.deepEqual(proofs[0].payload, original.local.attempts[0].data.payload);
    assert.deepEqual(proofs[1].payload, secondOriginal.local.attempts[1].data.payload);
    assert.equal(remote.writes.length, 2); assert.deepEqual(remote.ids, ['item-primary']); assert.deepEqual(remote.groups, []);
    assert.deepEqual(remote.masters['item-primary'].metadata.keywords, ['original third keyword']);
    const successful = structuredClone(remote); remote = initial(); absent = true;
    const absentFolder = await run(randomUUID(), 'merge-absent-start', true);
    const absentOriginal = JSON.parse(fs.readFileSync(path.join(absentFolder, 'boundary-merge-absent-start.json'), 'utf8'));
    await run(path.basename(absentFolder), 'merge-absent-resume'); unknown(absentFolder, absentOriginal);
    assert.deepEqual(remote.ids, initial().ids); assert.deepEqual(remote.groups, initial().groups);
    assert.equal(new Set(runs.map(r => r.process_id)).size, 12, 'Every crash/readback phase must use a new real process');
    const evidence = { passed: true, synthetic: true, full_process_and_webview_restart: true, runs, merge_workspace: folder, absent_workspace: absentFolder, successful_remote: successful, absent_remote: remote };
    fs.writeFileSync(path.join(folder, 'merge-restart-acceptance.json'), JSON.stringify(evidence, null, 2));
    console.log('Native merge restart PASS: original complete intent and source -> independent server acceptance -> owned app/WebView2 killed -> fresh startup blocks writes -> current SA, complete pool and master readback -> atomic verification. Third match continues through a second independently confirmed merge. Two saves total; SA remains pending.');
    console.log('Changed master/pool/SA/comparison/matched IDs/threshold and unsaved merge stay unknown without resending. Evidence:', path.join(folder, 'merge-restart-acceptance.json'));
  } finally { for (const reply of replies) reply.destroy(); server.close(); server.closeAllConnections(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
