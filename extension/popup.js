document.getElementById("pair").addEventListener("click", async () => {
  const status = document.getElementById("status");
  try {
    const [tab] = await chrome.tabs.query({active: true, currentWindow: true});
    const result = await chrome.runtime.sendMessage({type: "pair", tabId: tab?.id,
      token: document.getElementById("token").value.trim()});
    status.textContent = result.ok ? "已连接此页。下载请回桌面点“下载待补论文 TXT（WOS）”；后台入库请先连接 SA 比对结果页，再绑定另开的数据导入与批次管理页。" : result.error;
    if (result.ok) document.getElementById("token").value = "";
  } catch (error) { status.textContent = error.message; }
});
document.getElementById("disconnect").addEventListener("click", async () => {
  const result = await chrome.runtime.sendMessage({type: "disconnect"});
  document.getElementById("status").textContent = result.ok ? "已断开。已发出的请求不能撤回，请核验页面结果。" : result.error;
});
for (const [id, role] of [["wos", "wosTabId"], ["import", "importTabId"]]) {
  document.getElementById(id).addEventListener("click", async () => {
    const status = document.getElementById("status");
    try {
      const [tab] = await chrome.tabs.query({active: true, currentWindow: true});
      const result = await chrome.runtime.sendMessage({type: "bind_workflow", role, tabId: tab?.id});
      status.textContent = result.ok ? (role === "wosTabId"
        ? "此页已用于 WOS 检索。保持标签页打开，回桌面下载论文信息 TXT。"
        : "此页已用于 TXT 入库。保持 SA 比对页和此页打开，回桌面点“检查 TXT 并导入”。") : result.error;
    } catch (error) { status.textContent = error.message; }
  });
}
for(const [id,type] of [["open-import","open_import"],["inspect","inspect_workflow"],["mute","toggle_wos_mute"]]) {
  document.getElementById(id).addEventListener("click",async()=>{
    const status=document.getElementById("status");
    try{
      const [tab]=await chrome.tabs.query({active:true,currentWindow:true});
      const result=await chrome.runtime.sendMessage({type,tabId:tab?.id});
      if(!result.ok){status.textContent=result.error;return;}
      if(result.data){
        const box=document.getElementById("diagnostics");box.hidden=false;box.value=JSON.stringify(result.data,null,2);
        box.style.width="100%";
        status.textContent=result.data.wos_error?"WOS 自身错误页：请先点网页顶部 Search 恢复，再继续。下方诊断可复制反馈。":"只读检查完成。下方内容可选中复制；不含检索框输入、账号或 Cookie。";
      }else status.textContent=result.message;
    }catch(error){status.textContent=error.message;}
  });
}
