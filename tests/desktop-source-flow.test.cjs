// Actual source driver and browser downloads on intercepted local fixtures.
// Reserved .invalid hosts are fulfilled here; no database/account is accessed.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require("playwright");
const script = fs.readFileSync(
  path.resolve(__dirname, "../desktop/src-tauri/browser/source-adapter.cjs"),
  "utf8",
);
const runSourceCommand = new Function(script + "\nreturn runSourceCommand;")();
const runCNKICommand = new Function(
  fs.readFileSync(
    path.resolve(__dirname, "../desktop/src-tauri/browser/cnki-adapter.cjs"),
    "utf8",
  ) + "\nreturn runCNKICommand;",
)();
const origin = "https://database.invalid";
const evidenceRoot = path.resolve(
  process.env.VALIDATION_ROOT ||
    path.join(__dirname, "../runtime/validation-0.2.0-20261012"),
);
fs.mkdirSync(evidenceRoot, { recursive: true });
const flow = {
  channel: "ei",
  search_url: "",
  input: "",
  search_button: "",
  result_rows: "",
  result_title: "",
  result_link: "",
  result_total: "",
  empty_result: "",
  detail_title: "",
  export_steps: [],
};
const detail =
  '<meta name="citation_title" content="Paper"><meta name="citation_doi" content="10.1234/example"><main><h1>Paper</h1>Full original source</main>';
const command = (action, more = {}) => ({
  action,
  expires: Date.now() + 15000,
  title: "Paper",
  doi: "10.1234/example",
  site: { channel: "ei", download_origins: [origin] },
  flow,
  formats: ["csv", "xlsx"],
  ...more,
});
(async () => {
  const browser = await chromium.launch({
    executablePath:
      process.env.TEST_BROWSER ||
      "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
    headless: true,
  });
  const context = await browser.newContext({ acceptDownloads: true });
  let html = "",
    passed = 0;
  await context.route("**/*", (r) =>
    r.fulfill({
      status: 200,
      contentType: "text/html; charset=utf-8",
      body: html,
    }),
  );
  const page = await context.newPage();
  const boot = async (body, url = origin + "/record") => {
    html = "<!doctype html><html><head></head><body>" + body + "</body></html>";
    await page.goto(url);
  };
  const call = async (action, more) =>
    page.evaluate(runSourceCommand, command(action, more));
  const pass = (name) => {
    passed++;
    console.log("PASS " + name);
  };
  try {
    await boot(
      '<form><input type="search"><button type="button" onclick="window.queries=(window.queries||0)+1">搜索</button></form>',
    );
    const searched = await call("source_search");
    assert.equal(searched.ok, true);
    assert.equal(await page.locator("input").inputValue(), "Paper");
    assert.equal(await page.evaluate(() => window.queries), 1);
    pass("one query submitted using its associated form");
    await boot(
      '<form><input type="search"><button type="button">搜索</button><button type="button">搜索</button></form>',
    );
    const ambiguous = await call("source_search");
    assert.equal(ambiguous.ok, false);
    assert.equal(ambiguous.submitted, false);
    pass("ambiguous search controls never click");
    await boot(
      '<input type="search" value="Paper"><div class="result-count">1</div><article><h2><a href="/record">Paper</a></h2></article>',
      origin + "/search",
    );
    const results = await call("source_results");
    assert.equal(results.ok, true);
    assert.equal(results.data.complete, true);
    assert.deepEqual(results.data.records, [
      { title: "Paper", url: origin + "/record" },
    ]);
    pass("complete range returns exact titles and canonical URLs");
    await boot(
      '<input type="search" value="Paper"><div class="result-count">20</div><article><h2><a href="/record">Paper</a></h2></article>',
      origin + "/search",
    );
    const thin = await call("source_results");
    assert.equal(thin.ok, false);
    assert.equal(thin.code, "SOURCE_SELECTION_REQUIRED");
    pass("one rendered page never substitutes for twenty results");
    await boot(
      '<input type="search" value="Another"><div class="result-count">1</div><article><h2><a href="/record">Paper</a></h2></article>',
      origin + "/search",
    );
    assert.equal((await call("source_results")).code, "INPUT_CHANGED");
    pass("results must correspond to the dispatched query");
    await boot(
      '<input type="search" value="Paper"><p class="no-result">No records</p>',
      origin + "/search",
    );
    const empty = await call("source_results");
    assert.equal(empty.ok, true);
    assert.equal(empty.data.total, 0);
    pass("actual empty result is recorded separately");
    await boot(
      detail.replace(
        "</main>",
        '<meta name="citation_author" content="Alice"><meta name="citation_author" content="Bob"><meta name="citation_custom" content="untouched"><script type="application/ld+json">{"@type":"ScholarlyArticle","headline":"Paper","unknown":[1,2]}</script><table><tr><th>申请人</th><td>Original Applicant</td></tr><tr><th>未知原字段</th><td>原值完整保留</td></tr></table></main>',
      ),
    );
    const captured = await call("source_capture");
    assert.equal(captured.ok, true);
    assert.deepEqual(captured.data.fields.authors, ["Alice", "Bob"]);
    assert.equal(
      captured.data.fields.patent_applicants[0],
      "Original Applicant",
    );
    assert(
      captured.data.metas.some(
        (m) => m.name === "citation_custom" && m.value === "untouched",
      ),
    );
    assert.equal(JSON.parse(captured.data.jsonld[0]).unknown[1], 2);
    assert(captured.data.visible_text.includes("原值完整保留"));
    pass(
      "authors, unknown fields, raw JSON-LD and full visible text survive capture",
    );
    await boot(detail.replaceAll("Paper", "Corrected title"));
    assert.equal(
      (await call("source_capture")).code,
      "SOURCE_IDENTITY_CONFLICT",
    );
    const corrected = await call("source_capture", { manual_identity: true });
    assert.equal(corrected.ok, true);
    assert.equal(corrected.data.title, "Corrected title");
    pass("corrected title needs the actual matching DOI");
    await boot(
      detail
        .replaceAll("Paper", "Corrected title")
        .replace("10.1234/example", "10.1234/wrong"),
    );
    assert.equal(
      (await call("source_capture", { manual_identity: true })).code,
      "SOURCE_IDENTITY_CONFLICT",
    );
    pass("manual capture cannot bypass a different DOI");
    for (const [body, code] of [
      [
        "<h1>A timeout occurred</h1>Error code 524",
        "SOURCE_SERVICE_UNAVAILABLE",
      ],
      ['<input type="password">', "AUTH_REQUIRED"],
    ]) {
      await boot(body);
      assert.equal((await call("source_ping")).code, code);
      pass(code + " never becomes an empty result");
    }
    await boot(
      "<h1>请完成安全验证</h1>向右滑动完成验证",
      origin + "/verify/home",
    );
    assert.equal((await call("source_ping")).code, "AUTH_REQUIRED");
    pass(
      "redirected slider verification is classified before waiting for a form",
    );
    await boot('<div id="tCaptchaDyMainWrap">拖动下方拼图完成验证</div>');
    assert.equal((await call("source_ping")).code, "AUTH_REQUIRED");
    pass("visible CNKI puzzle overlay blocks queries");
    await boot(
      '<div id="tCaptchaDyMainWrap" style="position:absolute;top:-1000000px">拖动下方拼图完成验证</div>',
    );
    assert.equal((await call("source_ping")).ok, true);
    pass("completed offscreen challenge does not block the restored page");
    for (const [body, url] of [
      [
        "<h1>请完成安全验证</h1>向右滑动完成验证",
        "https://kns.cnki.net/verify/home",
      ],
      [
        '<div id="tCaptchaDyMainWrap">拖动下方拼图完成验证</div>',
        "https://kns.cnki.net/kns8s/defaultresult/index?kw=Paper",
      ],
    ]) {
      await boot(body, url);
      assert.equal(
        (await page.evaluate(runCNKICommand, command("cnki_results"))).code,
        "AUTH_REQUIRED",
      );
      pass("specialized CNKI guard handles " + new URL(url).pathname);
    }
    await boot(
      "<main>抱歉，暂无数据，请稍后重试。</main>",
      "https://kns.cnki.net/kns8s/defaultresult/index?kw=Paper&korder=TI",
    );
    const serviceFailure = await page.evaluate(
      runCNKICommand,
      command("cnki_results"),
    );
    assert.equal(serviceFailure.code, "SOURCE_SERVICE_UNAVAILABLE");
    pass("CNKI temporary service failure is detected before result waiting");
    const sharedFailure = await call("source_results", {
      site: { channel: "cnki", download_origins: ["https://kns.cnki.net"] },
      flow: { ...flow, channel: "cnki" },
    });
    assert.equal(sharedFailure.code, "SOURCE_SERVICE_UNAVAILABLE");
    assert.equal(sharedFailure.submitted, false);
    pass(
      "shared source driver never converts CNKI retry message into zero hits",
    );
    await boot(detail, "https://foreign.invalid/record");
    assert.equal((await call("source_capture")).code, "INPUT_CHANGED");
    pass("foreign origin cannot use the saved source session");
    await boot(detail + '<button id="save">Unexpected label</button>');
    const mismatch = await call("source_export", {
      flow: {
        ...flow,
        export_steps: [
          { selector: "#save", action: "click", text: "下载 CSV" },
        ],
      },
    });
    assert.equal(mismatch.code, "SOURCE_FLOW_INVALID");
    assert.equal(mismatch.submitted, false);
    pass("export recipe requires the exact visible label");
    await boot(detail + '<button id="save">上传</button>');
    const unsafe = await call("source_export", {
      flow: {
        ...flow,
        export_steps: [{ selector: "#save", action: "click", text: "上传" }],
      },
    });
    assert.equal(unsafe.ok, false);
    assert.equal(unsafe.submitted, false);
    pass("source recipes cannot issue a platform upload");
    await boot(detail + '<button id="save">下载 CSV</button>');
    await page.evaluate(() =>
      document.getElementById("save").addEventListener("click", () => {
        window.exports = (window.exports || 0) + 1;
        const a = document.createElement("a");
        a.href = URL.createObjectURL(
          new Blob(["Title,Author,DOI\nPaper,Alice,10.1234/example\n"], {
            type: "text/csv",
          }),
        );
        a.download = "paper.csv";
        a.click();
      }),
    );
    const downloadEvent = page.waitForEvent("download", { timeout: 5000 });
    const exported = await call("source_export", {
      flow: {
        ...flow,
        export_steps: [
          { selector: "#save", action: "click", text: "下载 CSV" },
        ],
      },
    });
    assert.equal(exported.ok, true);
    const download = await downloadEvent;
    assert.equal(download.suggestedFilename(), "paper.csv");
    assert.equal(await download.failure(), null);
    const output = path.join(evidenceRoot, "source-flow.csv");
    await download.saveAs(output);
    assert.equal(
      fs.readFileSync(output, "utf8"),
      "Title,Author,DOI\nPaper,Alice,10.1234/example\n",
    );
    assert.equal(await page.evaluate(() => window.exports), 1);
    pass(
      "configured original export sends one click and receives exact complete bytes",
    );
    const evidence = {
      passed: true,
      cases: passed,
      scope:
        "fully intercepted browser fixtures (reserved domains and official CNKI route shapes); no live requests",
      real_sites_accessed: false,
      production_native_source_callbacks_verified: false,
    };
    fs.writeFileSync(
      path.join(evidenceRoot, "source-flow-results.json"),
      JSON.stringify(evidence, null, 2),
    );
    console.log(
      "Source driver: " +
        passed +
        " cases passed. Browser/DOM fixture evidence; not live database acceptance.",
    );
  } finally {
    await context.close();
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
