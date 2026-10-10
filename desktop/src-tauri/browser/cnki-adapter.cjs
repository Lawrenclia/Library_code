/* Independent implementation against public CNKI detail export controls.
 * Protocol reference: l0o0/translators_CN CNKI.js, getItemFromAPI (2026-09-29).
 * Uses the page's own session; no access to cookies, credentials or private APIs.
 */
async function runCNKICommand(command) {
  const fail = (code, message) => {
    throw Object.assign(new Error(message), { code });
  };
  let timer;
  try {
    const page = new URL(location.href);
    const official = (u) =>
      u.protocol === "https:" &&
      !u.port &&
      !u.username &&
      !u.password &&
      ["kns.cnki.net", "kns8.cnki.net"].includes(u.hostname);
    if (command.action !== "cnki_capture" || !official(page))
      fail(
        "PAGE_UNSUPPORTED",
        "请进入 CNKI 官方单篇详情页。代理访问可使用网页原始导出。",
      );
    const oneValue = (selector) => {
      const nodes = document.querySelectorAll(selector);
      if (nodes.length !== 1 || !nodes[0].value?.trim())
        fail(
          "PAGE_UNSUPPORTED",
          "当前不是支持的单篇详情页。请先打开目标论文，再获取题录；也可手动导出 Excel/TXT。",
        );
      return nodes[0].value;
    };
    const endpoint = new URL(oneValue("#export-url"), page);
    const exportId = oneValue("#export-id");
    if (
      !official(endpoint) ||
      endpoint.origin !== page.origin ||
      !endpoint.pathname.toLowerCase().endsWith("/getexport") ||
      exportId.length > 2000
    )
      fail(
        "PAGE_UNSUPPORTED",
        "详情页导出入口不受支持，请使用网页的原始导出按钮。",
      );
    const remaining = Math.min(25000, command.expires - Date.now() - 3000);
    if (remaining <= 0) fail("PAGE_TIMEOUT", "获取题录已到时。");
    const abort = new AbortController();
    timer = setTimeout(() => abort.abort(), remaining);
    const response = await fetch(endpoint.href, {
      method: "POST",
      credentials: "same-origin",
      redirect: "error",
      signal: abort.signal,
      headers: {
        "Content-Type": "application/x-www-form-urlencoded;charset=UTF-8",
      },
      body: new URLSearchParams({
        filename: exportId,
        uniplatform: "NZKPT",
        displaymode: "GBTREFER,elearning,EndNote",
      }),
    });
    if (!response.ok)
      fail(
        "CNKI_EXPORT_FAILED",
        `CNKI 导出返回 ${response.status}，请在窗口确认登录、权限或验证码。`,
      );
    const reader = response.body?.getReader();
    if (!reader) fail("CNKI_EXPORT_FAILED", "CNKI 导出响应为空。");
    const chunks = [];
    let size = 0;
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      size += value.length;
      if (size > 200000) {
        await reader.cancel();
        fail("CNKI_EXPORT_FAILED", "题录响应过大，请使用单篇导出。");
      }
      chunks.push(value);
    }
    const bytes = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.length;
    }
    const responseText = new TextDecoder("utf-8", { fatal: true }).decode(
      bytes,
    );
    let data;
    try {
      data = JSON.parse(responseText);
    } catch {
      fail(
        "AUTH_REQUIRED",
        "CNKI 返回了登录或验证页面，请在内置窗口完成后再获取。",
      );
    }
    const texts = [];
    const collect = (value, depth = 0) => {
      if (depth > 12) fail("CNKI_EXPORT_FAILED", "CNKI 响应结构不受支持。");
      if (typeof value === "string") {
        // Parse HTML in an inert document. Never inject returned markup into a page.
        const plain =
          new DOMParser().parseFromString(
            value.replace(/<br\s*\/?\s*>/gi, "\n"),
            "text/html",
          ).body.textContent || "";
        if (/^\s*(%0|RT)[ \t]/.test(plain)) texts.push(plain.trim());
      } else if (Array.isArray(value))
        value.forEach((v) => collect(v, depth + 1));
      else if (value && typeof value === "object")
        Object.values(value).forEach((v) => collect(v, depth + 1));
    };
    collect(data.data);
    const preferred = texts.filter((text) => /^%0[ \t]/.test(text));
    const unique = [...new Set(preferred.length ? preferred : texts)];
    if (unique.length !== 1)
      fail(
        "CNKI_EXPORT_FAILED",
        "导出未唯一返回完整 EndNote/RefWorks 题录。请用网页导出 Excel/TXT，不会用引用短句代替完整记录。",
      );
    if (location.href !== page.href || oneValue("#export-id") !== exportId)
      fail("INPUT_CHANGED", "获取期间详情页变化，请重新获取。");
    return {
      ok: true,
      data: {
        page_url: page.href,
        export_url: endpoint.href,
        export_id: exportId,
        text: unique[0],
        response_text: responseText,
      },
    };
  } catch (error) {
    return {
      ok: false,
      code:
        error.code ||
        (error.name === "AbortError" ? "PAGE_TIMEOUT" : "CNKI_EXPORT_FAILED"),
      error:
        error.name === "AbortError"
          ? "CNKI 获取题录超时，请检查页面；本次没有自动重试。"
          : String(error.message || error),
    };
  } finally {
    clearTimeout(timer);
  }
}
