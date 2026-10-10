const $ = id => document.getElementById(id);
let working = false;
async function refreshStatus() {
  const result = await chrome.runtime.sendMessage({type:'popup_status'});
  if (!result?.ok) throw new Error(result?.error || '无法读取连接状态，请重新打开插件。');
  const d = result.data;
  $('connection').textContent = !d.paired ? '尚未配对 · 请先在桌面助手获取配对码' :
    `${d.busy ? '正在执行任务' : '已配对'} · ${d.primary ? '主工作页可用' : '主工作页已关闭或切换'}\nWOS：${d.wos ? '已绑定' : '未绑定或已切换'} · 导入页：${d.import ? '已绑定' : '未绑定或已切换'}`;
  $('connection').dataset.state = d.paired && d.primary ? 'ready' : 'pending';
  $('connection').title = `插件版本 ${d.version}`;
}
async function run(button, operation) {
  if (working) return;
  working = true;
  const label = button.textContent;
  const buttons = [...document.querySelectorAll('button')];
  buttons.forEach(b => b.disabled = true);
  button.textContent = '处理中…';
  try { await operation(); }
  catch (error) { $('status').textContent = error.message || '操作失败，请重试。'; }
  finally {
    button.textContent = label;
    buttons.forEach(b => b.disabled = false);
    working = false;
  }
}
async function send(type, extra={}) {
  const [tab] = await chrome.tabs.query({active:true,currentWindow:true});
  const result = await chrome.runtime.sendMessage({type,tabId:tab?.id,...extra});
  if (!result?.ok) throw new Error(result?.error || '插件未返回结果，请重新打开弹窗。');
  return result;
}
function bind(id, action) { $(id).addEventListener('click', () => run($(id),action)); }
bind('pair', async () => {
  const token = $('token').value.trim();
  if (!token) { $('token').focus(); throw new Error('请先粘贴桌面助手提供的配对码。'); }
  await send('pair',{token});
  $('token').value = '';
  $('status').textContent = '配对成功。WOS 下载请回桌面点击“下载待补论文 TXT（WOS）”，并保持工作页打开。';
  await refreshStatus();
});
$('token').addEventListener('keydown', event => {
  if (event.key === 'Enter') { event.preventDefault(); $('pair').click(); }
});
bind('disconnect', async () => {
  await send('disconnect');
  $('status').textContent = '已断开。已发出的请求不能撤回，请核验页面结果。';
  await refreshStatus();
});
bind('refresh',refreshStatus);
for (const [id,role] of [['wos','wosTabId'],['import','importTabId']]) {
  bind(id, async () => {
    await send('bind_workflow',{role});
    $('status').textContent = role === 'wosTabId' ? '工作页已绑定，用于 WOS 检索。保持标签页打开，回桌面继续。' : '工作页已绑定，用于 TXT 入库。保持标签页打开，回桌面继续。';
    await refreshStatus();
  });
}
for (const [id,type] of [['open-import','open_import'],['inspect','inspect_workflow'],['mute','toggle_wos_mute']]) {
  bind(id,async () => {
    const result = await send(type);
    if (result.data) {
      $('diagnostics').hidden = false;
      $('diagnostics').value = JSON.stringify(result.data,null,2);
      $('copy-diagnostics').hidden = false;
      $('status').textContent = result.data.wos_error ? 'WOS 显示错误页：先用网页顶部 Search 恢复，再继续。可复制诊断反馈。' : '只读检查完成，可复制诊断信息反馈。';
    } else $('status').textContent = result.message;
  });
}
bind('copy-diagnostics',async () => {
  try {
    await navigator.clipboard.writeText($('diagnostics').value);
    $('status').textContent = '诊断信息已复制。';
  } catch {
    $('diagnostics').focus();
    $('diagnostics').select();
    $('status').textContent = '自动复制不可用，已选中诊断信息，请按 Ctrl+C 复制。';
  }
});
refreshStatus().catch(error => { $('connection').textContent = error.message; });
