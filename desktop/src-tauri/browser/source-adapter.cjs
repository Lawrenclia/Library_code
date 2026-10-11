/* Database controls are read from the current page, scoped to a saved source
 * window. Recipes contain selectors/text only, never executable scripts. */
async function runSourceCommand(c) {
  const stop = (code, message, submitted = false) => {
    throw Object.assign(new Error(message), { code, submitted });
  };
  let submitted = false;
  try {
    const start = location.href;
    const norm = (s) =>
      String(s || "")
        .normalize("NFKC")
        .toLowerCase()
        .trim()
        .replace(/\s+/gu, " ");
    const visible = (e) =>
      !!e?.getClientRects().length &&
      getComputedStyle(e).visibility !== "hidden";
    const text = (e) =>
      (
        e?.getAttribute("aria-label") ||
        (e?.labels?.length === 1 ? e.labels[0].textContent : "") ||
        e?.textContent ||
        e?.value ||
        ""
      ).trim();
    const query = (s, root = document) => {
      try {
        return [...root.querySelectorAll(s)].filter(visible);
      } catch {
        stop(
          "SOURCE_FLOW_INVALID",
          "页面定位条件无效，请调整渠道流程。",
          submitted,
        );
      }
    };
    const one = (s, root = document) => {
      const nodes = query(s, root);
      if (nodes.length !== 1)
        stop(
          "PAGE_UNSUPPORTED",
          "页面控件不是唯一可见项，未点击。请保存本渠道实际定位条件。",
          submitted,
        );
      return nodes[0];
    };
    const guard = () => {
      const page = new URL(location.href);
      if (
        !c.site.download_origins.includes(page.origin) ||
        location.href !== start
      )
        stop(
          "INPUT_CHANGED",
          "获取期间页面离开原范围，请检查来源窗口。",
          submitted,
        );
      if (Date.now() > c.expires - 1500)
        stop("PAGE_TIMEOUT", "来源页面等待超时，未据此判定无记录。", submitted);
      if (
        /A timeout occurred|Error code\s*52[024]|Host Error/i.test(
          document.body?.innerText?.slice(0, 1800) || "",
        )
      )
        stop(
          "SOURCE_SERVICE_UNAVAILABLE",
          "数据库来源服务器超时，请恢复后继续。",
          submitted,
        );
      if (
        query(
          'input[type="password"],input[id*="captcha" i],input[name*="captcha" i],iframe[src*="captcha" i]',
        ).length
      )
        stop("AUTH_REQUIRED", "请在来源窗口完成登录或验证后继续。", submitted);
    };
    const settle = () => new Promise((r) => setTimeout(r, 250));
    const ready = async () => {
      while (
        document.readyState !== "complete" ||
        query('[aria-busy="true"],[role="progressbar"]').length
      ) {
        guard();
        await settle();
      }
      guard();
    };
    const fieldTitle = () => {
      const values = [
        ...document.querySelectorAll(
          'meta[name="citation_title" i],meta[name="dc.title" i]',
        ),
      ]
        .map((e) => e.content?.trim())
        .filter(Boolean);
      if (c.flow.detail_title)
        values.push(one(c.flow.detail_title).textContent.trim());
      else if (!values.length)
        values.push(...query("h1").map((e) => e.textContent.trim()));
      const distinct = [...new Set(values.map(norm))];
      const doiNorm = (s) =>
        norm(s)
          .replace(/^https?:\/\/(?:dx\.)?doi\.org\//i, "")
          .replace(/^doi:\s*/i, "");
      const pageDois = [
        ...document.querySelectorAll('meta[name="citation_doi" i]'),
      ]
        .map((m) => doiNorm(m.content))
        .filter(Boolean);
      const sameDoi =
        c.manual_identity &&
        c.doi &&
        pageDois.length > 0 &&
        pageDois.every((d) => d === doiNorm(c.doi));
      if (distinct.length !== 1 || (distinct[0] !== norm(c.title) && !sameDoi))
        stop(
          "SOURCE_IDENTITY_CONFLICT",
          "详情页没有唯一且与名单一致的题名，未导出或采纳。",
          submitted,
        );
      return values[0];
    };
    await ready();
    if (c.action === "source_ping")
      return { ok: true, data: { page_url: start, complete: true } };
    if (c.action === "source_search") {
      const input = c.flow.input
        ? one(c.flow.input)
        : one(
            'input[type="search"],input[placeholder*="题名"],input[placeholder*="篇名"],input[placeholder*="检索"],input[placeholder*="搜索"],input[aria-label*="Search" i]',
          );
      if (
        !["INPUT", "TEXTAREA"].includes(input.tagName) ||
        input.disabled ||
        input.readOnly
      )
        stop("PAGE_UNSUPPORTED", "当前不是可用检索输入框。", false);
      const proto =
        input.tagName === "TEXTAREA"
          ? HTMLTextAreaElement.prototype
          : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value").set.call(input, c.title);
      input.dispatchEvent(new Event("input", { bubbles: true }));
      input.dispatchEvent(new Event("change", { bubbles: true }));
      const root =
        input.form ||
        input.closest('[role="search"]') ||
        input.parentElement.parentElement;
      const buttons = c.flow.search_button
        ? [one(c.flow.search_button)]
        : query('button,[role="button"],input[type="submit"]', root).filter(
            (e) => /^(?:检索|搜索|search|查询)$/i.test(text(e)),
          );
      if (
        buttons.length !== 1 ||
        buttons[0].disabled ||
        input.value !== c.title
      )
        stop("PAGE_UNSUPPORTED", "没有唯一关联的检索按钮，未提交。", false);
      guard();
      submitted = true;
      buttons[0].click();
      return {
        ok: true,
        data: { submitted: true, query: c.title, page_url: start },
      };
    }
    if (c.action === "source_results") {
      let signature = "",
        stable = Date.now();
      for (;;) {
        guard();
        const inputs = c.flow.input
          ? query(c.flow.input)
          : query(
              'input[type="search"],input[placeholder*="题名"],input[placeholder*="篇名"],input[placeholder*="检索"],input[placeholder*="搜索"],input[aria-label*="Search" i]',
            );
        const queryMatches =
          c.query_url === start ||
          (inputs.length === 1 && norm(inputs[0].value) === norm(c.title));
        if (!queryMatches)
          stop(
            "INPUT_CHANGED",
            "无法核对本次结果对应的检索词。请保存实际检索框定位条件。",
            false,
          );
        const rows = query(
          c.flow.result_rows ||
            'article,[role="article"],table tbody tr,.result-item,.result-list-item',
        );
        const records = rows.map((row) => {
          const titles = query(
            c.flow.result_title ||
              "h2 a[href],h3 a[href],.title a[href],a[title][href]",
            row,
          );
          const links = c.flow.result_link
            ? query(c.flow.result_link, row)
            : titles;
          if (titles.length !== 1 || links.length !== 1) return null;
          const title = (
            titles[0].getAttribute("title") || titles[0].textContent
          ).trim();
          const url = new URL(links[0].getAttribute("href"), start);
          return title && c.site.download_origins.includes(url.origin)
            ? { title, url: url.href }
            : null;
        });
        const totals = c.flow.result_total
          ? query(c.flow.result_total)
          : query(
              "[data-result-count],.result-count,.total-count,.search-result-count",
            );
        const nums =
          totals.length === 1
            ? (totals[0].textContent.match(/\d[\d,，]*/g) || []).map((s) =>
                Number(s.replace(/[,，]/g, "")),
              )
            : [];
        const empty = c.flow.empty_result
          ? query(c.flow.empty_result).length === 1
          : query('[role="status"],.no-result,.no-results,.empty-result').some(
              (e) =>
                /没有|未检索到|无结果|no results|no records/i.test(
                  e.textContent,
                ),
            );
        const busy =
          document.readyState !== "complete" ||
          query('[aria-busy="true"],[role="progressbar"]').length > 0;
        const next = JSON.stringify({ records, nums, empty, busy });
        if (next !== signature) {
          signature = next;
          stable = Date.now();
        }
        if (Date.now() - stable >= 1200 && !busy) {
          if (!rows.length && empty)
            return {
              ok: true,
              data: {
                page_url: start,
                query: c.title,
                records: [],
                total: 0,
                complete: true,
              },
            };
          if (
            rows.length &&
            records.every(Boolean) &&
            nums.length === 1 &&
            nums[0] === rows.length
          )
            return {
              ok: true,
              data: {
                page_url: start,
                query: c.title,
                records,
                total: nums[0],
                complete: true,
              },
            };
          if (Date.now() - stable > 3000)
            stop(
              "SOURCE_SELECTION_REQUIRED",
              "结果数量或完整范围不能核对。请缩小范围或进入已核对详情；不会选择第一页首篇。",
              false,
            );
        }
        await settle();
      }
    }
    const title = fieldTitle();
    if (c.action === "source_capture") {
      const fields = {},
        metas = [],
        jsonld = [];
      const add = (key, value) => {
        if (typeof value !== "string" || !value.trim()) return;
        (fields[key] ||= []).push(value.trim());
      };
      const names = {
        citation_author: "authors",
        citation_author_institution: "affiliations",
        citation_date: "year",
        citation_publication_date: "year",
        citation_journal_title: "publication",
        citation_conference_title: "publication",
        citation_book_title: "publication",
        citation_publisher: "publisher",
        citation_volume: "volume",
        citation_issue: "issue",
        citation_firstpage: "pages",
        citation_lastpage: "pages",
        citation_article_number: "article_number",
        citation_doi: "doi",
        citation_abstract: "abstract",
        citation_keywords: "keywords",
        citation_issn: "issn",
        citation_isbn: "isbn",
        "dc.creator": "authors",
        "dc.description": "abstract",
        "dc.publisher": "publisher",
        "dc.date": "year",
      };
      for (const m of document.querySelectorAll("meta[name],meta[property]")) {
        const name = m.getAttribute("name") || m.getAttribute("property"),
          value = m.getAttribute("content") || "";
        if (/^(citation_|dc\.|dcterms\.|prism\.|og:)/i.test(name)) {
          metas.push({ name, value });
          if (names[name.toLowerCase()]) add(names[name.toLowerCase()], value);
        }
      }
      const entities = [];
      const walk = (v) => {
        if (!v || typeof v !== "object") return;
        if (Array.isArray(v)) {
          v.forEach(walk);
          return;
        }
        entities.push(v);
        if (v["@graph"]) walk(v["@graph"]);
        if (v.mainEntity) walk(v.mainEntity);
      };
      for (const s of document.querySelectorAll(
        'script[type="application/ld+json"]',
      )) {
        const raw = s.textContent;
        jsonld.push(raw);
        try {
          walk(JSON.parse(raw));
        } catch {
          /* Raw invalid structured metadata remains in the provenance. */
        }
      }
      const articles = entities.filter(
        (v) =>
          norm(v.headline || v.name) === norm(c.title) &&
          [v["@type"]]
            .flat()
            .some((t) =>
              /^(ScholarlyArticle|Article|NewsArticle|TechArticle|Book|Chapter|Patent|Report|Thesis|Dataset|CreativeWork)$/i.test(
                t,
              ),
            ),
      );
      if (articles.length > 1)
        stop(
          "SOURCE_IDENTITY_CONFLICT",
          "详情结构化数据有多份同名成果，请核对。",
          false,
        );
      if (articles.length === 1) {
        const a = articles[0];
        for (const author of [a.author || []].flat()) {
          add("authors", typeof author === "string" ? author : author.name);
          for (const aff of [author?.affiliation || []].flat())
            add("affiliations", typeof aff === "string" ? aff : aff.name);
        }
        add("year", a.datePublished);
        add("abstract", a.abstract || a.description);
        for (const kw of [a.keywords || []].flat()) add("keywords", kw);
        add(
          "publication",
          typeof a.isPartOf === "string" ? a.isPartOf : a.isPartOf?.name,
        );
        add(
          "publisher",
          typeof a.publisher === "string" ? a.publisher : a.publisher?.name,
        );
        add("pages", a.pagination);
        add("article_number", a.articleNumber);
        for (const id of [a.identifier || []].flat())
          if (typeof id === "object" && /doi/i.test(id.propertyID || id.name))
            add("doi", id.value);
      }
      // Keep actual visible article fields (including unknown labels) instead of
      // synthesizing authors, units, publication dates or patent attributes.
      const root = query('main,article,[role="main"]');
      const body = root.length === 1 ? root[0] : document.body;
      const visible_text = body?.innerText || "";
      if (
        visible_text.length > 250000 ||
        JSON.stringify({ metas, jsonld }).length > 500000
      )
        stop(
          "SOURCE_TOO_LARGE",
          "详情原文过大，不会截断采用，请导出原始文件。",
          false,
        );
      const labelled = [];
      for (const dt of body.querySelectorAll("dt")) {
        const dd = dt.nextElementSibling;
        if (dd?.tagName === "DD" && visible(dt) && visible(dd))
          labelled.push({
            label: dt.textContent.trim(),
            value: dd.textContent.trim(),
          });
      }
      for (const row of body.querySelectorAll("tr")) {
        const cells = [...row.children].filter((e) =>
          /^(TH|TD)$/.test(e.tagName),
        );
        if (cells.length === 2 && visible(row))
          labelled.push({
            label: cells[0].textContent.trim(),
            value: cells[1].textContent.trim(),
          });
      }
      const labels = {
        作者: "authors",
        作者单位: "affiliations",
        机构: "affiliations",
        出版年: "year",
        年份: "year",
        来源: "publication",
        刊名: "publication",
        期刊: "publication",
        出版社: "publisher",
        摘要: "abstract",
        关键词: "keywords",
        页码: "pages",
        卷: "volume",
        期: "issue",
        doi: "doi",
        申请人: "patent_applicants",
        发明人: "patent_inventors",
        申请号: "patent_application_number",
        公开号: "patent_publication_number",
        申请日期: "patent_application_date",
      };
      for (const f of labelled) {
        const key = labels[norm(f.label).replace(/[:：]$/u, "")];
        if (key) add(key, f.value);
      }
      for (const [key, values] of Object.entries(fields))
        fields[key] = [...new Set(values)];
      guard();
      return {
        ok: true,
        data: {
          page_url: start,
          title,
          fields,
          metas,
          jsonld,
          labelled,
          visible_text,
          complete: true,
        },
      };
    }
    if (c.action !== "source_export")
      stop("INVALID_ACTION", "未知来源操作。", false);
    const checked = (node, expected) => {
      if (norm(text(node)) !== norm(expected))
        stop(
          "SOURCE_FLOW_INVALID",
          "导出控件实际文字与保存步骤不符，未点击。",
          submitted,
        );
      if (node.disabled || node.getAttribute("aria-disabled") === "true")
        stop("PAGE_UNSUPPORTED", "导出控件不可用。", submitted);
    };
    const exportSafe = (s) =>
      !/删除|移除|提交|上传|导入|购买|支付|delete|remove|upload|import|purchase/i.test(
        s,
      );
    if (c.flow.export_steps.length) {
      for (const step of c.flow.export_steps) {
        while (!query(step.selector).length) {
          guard();
          await settle();
        }
        guard();
        fieldTitle();
        const node = one(step.selector);
        if (!exportSafe(step.text))
          stop(
            "SOURCE_FLOW_INVALID",
            "来源流程只允许检索与导出控件。",
            submitted,
          );
        if (step.action === "select") {
          if (
            node.tagName !== "SELECT" ||
            node.disabled ||
            node.getAttribute("aria-disabled") === "true"
          )
            stop("PAGE_UNSUPPORTED", "此步骤不是下拉选择。", submitted);
          const choices = [...node.options].filter(
            (o) => norm(o.textContent) === norm(step.text),
          );
          if (choices.length !== 1)
            stop(
              "PAGE_UNSUPPORTED",
              "格式或完整记录选项不能唯一确认。",
              submitted,
            );
          node.value = choices[0].value;
          node.dispatchEvent(new Event("change", { bubbles: true }));
          if (node.value !== choices[0].value)
            stop("PAGE_UNSUPPORTED", "导出选项没有生效。", submitted);
        } else {
          checked(node, step.text);
          if (step.action === "check") {
            if (node.tagName !== "INPUT" || node.type !== "checkbox")
              stop("PAGE_UNSUPPORTED", "此步骤不是复选框。", submitted);
            if (!node.checked) {
              submitted = true;
              node.click();
            }
            if (!node.checked)
              stop("PAGE_UNSUPPORTED", "选中状态未生效。", submitted);
          } else {
            submitted = true;
            node.click();
          }
        }
        await settle();
      }
      return { ok: true, data: { page_url: start, title, issued: true } };
    }
    // Default semantic recipe reads exact visible export labels. No PDF/fulltext
    // controls, arbitrary first buttons, private endpoints or generated files.
    const controls = () =>
      query('button,a,[role="button"],[role="menuitem"],input[type="button"]');
    const clickText = (pattern, root = document) => {
      const candidates = controls().filter(
        (e) => root.contains(e) && pattern.test(text(e)),
      );
      if (candidates.length !== 1)
        stop(
          "PAGE_UNSUPPORTED",
          "没有唯一的原始题录导出控件，请保存本渠道导出步骤。",
          submitted,
        );
      if (
        candidates[0].disabled ||
        candidates[0].getAttribute("aria-disabled") === "true"
      )
        stop("PAGE_UNSUPPORTED", "导出控件不可用。", submitted);
      guard();
      submitted = true;
      candidates[0].click();
    };
    clickText(
      /^(?:导出|导出题录|题录导出|导出引用|引用导出|export|export citations|export records|citation export)$/i,
    );
    await settle();
    const formatPatterns = [];
    if (c.formats.includes("xlsx") || c.formats.includes("xls"))
      formatPatterns.push(
        /^(?:excel|excel file|excel格式|导出excel|微软excel)$/i,
      );
    if (c.formats.includes("csv"))
      formatPatterns.push(/^(?:csv|csv file|导出csv)$/i);
    if (c.formats.includes("txt"))
      formatPatterns.push(
        /^(?:txt|纯文本|plain text|文本|文本文件|制表符分隔文件|tab.delimited.*)$/i,
      );
    let picked = false;
    while (!picked) {
      guard();
      for (const pattern of formatPatterns) {
        const choices = controls().filter((e) => pattern.test(text(e)));
        if (choices.length === 1) {
          clickText(pattern);
          picked = true;
          break;
        }
      }
      if (!picked && Date.now() > c.expires - 5000)
        stop(
          "PAGE_UNSUPPORTED",
          "没有可确认的渠道原始导出格式，请保存实际步骤。",
          submitted,
        );
      if (!picked) await settle();
    }
    await settle();
    const dialogs = query('[role="dialog"],dialog,.el-dialog,.modal');
    if (dialogs.length === 1) {
      const d = dialogs[0];
      for (const select of query("select", d)) {
        const options = [...select.options].filter((o) =>
          /^(?:完整记录|全记录|full record|full record and cited references)$/i.test(
            o.textContent.trim(),
          ),
        );
        if (options.length === 1) {
          select.value = options[0].value;
          select.dispatchEvent(new Event("change", { bubbles: true }));
          if (select.value !== options[0].value)
            stop("PAGE_UNSUPPORTED", "完整记录选择未生效。", submitted);
        }
      }
      clickText(/^(?:导出|下载|export|download|save|保存)$/i, d);
    }
    return { ok: true, data: { page_url: start, title, issued: true } };
  } catch (e) {
    return {
      ok: false,
      code: e.code || "PAGE_UNSUPPORTED",
      error: String(e.message || e),
      submitted: e.submitted ?? submitted,
    };
  }
}
