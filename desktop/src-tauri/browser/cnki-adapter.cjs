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
    if (
      !["cnki_capture", "cnki_results"].includes(command.action) ||
      !official(page)
    )
      fail(
        "PAGE_UNSUPPORTED",
        "请进入 CNKI 官方单篇详情页。代理访问可使用网页原始导出。",
      );
    const norm = (s) =>
      String(s || "")
        .normalize("NFKC")
        .toLowerCase()
        .trim()
        .replace(/\s+/gu, " ");
    const visible = (node) =>
      !!node?.getClientRects().length &&
      getComputedStyle(node).visibility !== "hidden";
    const guard = () => {
      if (location.href !== page.href)
        fail("INPUT_CHANGED", "获取期间网页变化，请重新开始。");
      if (Date.now() >= command.expires - 2500)
        fail("PAGE_TIMEOUT", "CNKI 页面尚未加载完成，请在窗口核对后继续。");
      const heading = document.body?.innerText?.slice(0, 1600) || "";
      if (/A timeout occurred|Error code\s*52[024]|Host Error/i.test(heading))
        fail(
          "SOURCE_SERVICE_UNAVAILABLE",
          "CNKI 来源服务器超时，本次没有判定为检索无结果。",
        );
      if (
        [
          ...document.querySelectorAll(
            'input[type="password"], input[id*="captcha" i], input[name*="captcha" i], iframe[src*="captcha" i]',
          ),
        ].some(visible)
      )
        fail(
          "AUTH_REQUIRED",
          "请先在内置 CNKI 窗口完成登录或验证码，再继续获取。",
        );
    };
    const settle = () => new Promise((resolve) => setTimeout(resolve, 300));
    if (command.action === "cnki_results") {
      if (
        !command.title ||
        norm(page.searchParams.get("kw")) !== norm(command.title) ||
        !/\/defaultresult\//i.test(page.pathname)
      )
        fail("INPUT_CHANGED", "当前检索页不属于本篇题名，请重新按名单检索。");
      // Public result-table contract; never select an arbitrary first link.
      let signature = "",
        stableSince = 0;
      for (;;) {
        guard();
        const containers = document.querySelectorAll("#ModuleSearchResult");
        const container = containers.length === 1 ? containers[0] : null;
        const rows = container
          ? [...container.querySelectorAll(".result-table-list > tbody > tr")]
          : [];
        const records = rows.map((row) => {
          const links = row.querySelectorAll(".name a[href]");
          if (links.length !== 1) return null;
          const link = links[0];
          const title =
            link.getAttribute("title")?.trim() || link.textContent.trim();
          let target;
          try {
            target = new URL(link.getAttribute("href"), page);
          } catch {
            return null;
          }
          if (
            !title ||
            !official(target) ||
            !/^\/kcms2?\//i.test(target.pathname)
          )
            return null;
          return { title, url: target.href };
        });
        const text = container?.innerText || "";
        const counts = [
          ...text.matchAll(
            /(?:共(?:找到|检索到)?|找到|检索到)\s*([\d,，]+)\s*条(?:结果|记录|文献)?/g,
          ),
        ].map((m) => Number(m[1].replace(/[,，]/g, "")));
        const totals = [...new Set(counts)];
        const empty =
          rows.length === 0 &&
          /未(?:检索|搜索|查找)到|没有(?:找到|检索到|相关).*?(?:结果|文献)|无检索结果/.test(
            text,
          );
        const busy =
          container?.getAttribute("aria-busy") === "true" ||
          [
            ...(container?.querySelectorAll(
              '[aria-busy="true"], [role="progressbar"]',
            ) || []),
          ].some(visible);
        const next = JSON.stringify({ records, totals, empty, busy });
        if (next !== signature) {
          signature = next;
          stableSince = Date.now();
        }
        if (
          container &&
          document.readyState === "complete" &&
          !busy &&
          Date.now() - stableSince >= 1200
        ) {
          if (empty)
            return {
              ok: true,
              data: {
                page_url: page.href,
                query: command.title,
                complete: true,
                total: 0,
                records: [],
              },
            };
          // All hits must be present: a matching first page cannot prove uniqueness.
          if (
            totals.length === 1 &&
            totals[0] === rows.length &&
            records.every(Boolean) &&
            rows.length > 0
          )
            return {
              ok: true,
              data: {
                page_url: page.href,
                query: command.title,
                complete: true,
                total: totals[0],
                records,
              },
            };
          if (rows.length && totals.length === 1 && totals[0] > rows.length)
            fail(
              "SOURCE_SELECTION_REQUIRED",
              "检索结果包含多页，不能凭第一页选择论文。请缩小检索或打开核对后的单篇详情，再获取题录。",
            );
          if (rows.length && Date.now() - stableSince >= 3000)
            fail(
              "SOURCE_SELECTION_REQUIRED",
              "当前页面无法确认完整结果数量或论文链接，请在窗口打开核对后的单篇详情，再获取题录。",
            );
        }
        await settle();
      }
    }
    if (!/^\/kcms2?\//i.test(page.pathname))
      fail("PAGE_UNSUPPORTED", "请先打开 CNKI 单篇详情页，再获取题录。");
    // Wait for the navigated detail page instead of immediately treating a
    // partially loaded document as an unsupported page.
    for (;;) {
      guard();
      if (
        document.querySelectorAll("#export-url").length === 1 &&
        document.querySelectorAll("#export-id").length === 1
      )
        break;
      await settle();
    }
    if (command.title) {
      const titles = document.querySelectorAll(".wx-tit > h1");
      if (
        titles.length !== 1 ||
        norm(titles[0].textContent) !== norm(command.title)
      )
        fail(
          "SOURCE_IDENTITY_CONFLICT",
          "详情页题名与名单不一致，请核对目标论文。",
        );
    }
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
