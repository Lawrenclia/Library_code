import {
  computed,
  nextTick,
  onMounted,
  onUnmounted,
  reactive,
  ref,
  watch,
} from "vue";
import { callDesktop, type DesktopCommand } from "@/services/desktop";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Task,
  Failure,
  Workspace,
  Review,
  Template,
  MaterialValidation,
  DuplicateScan,
  DuplicatePrepared,
  IssueChecklist,
  MetadataPrepared,
  LibrarySearch,
} from "@/types";

export function useWorkbench() {
  const workspace = ref<Workspace>({
    tasks: [],
    root: "",
    running: false,
    paused: false,
    browsers: {},
    policy: "",
  });
  const page = ref("tasks"),
    owner = ref(""),
    search = ref(""),
    filter = ref("all"),
    selectedId = ref("");
  const message = ref("导入名单后，选择负责人开始处理。"),
    error = ref(false),
    pending = ref(false),
    detailTab = ref("review");
  const schemas = ref<Template[]>([]),
    templateId = ref(""),
    templateRow = ref(2),
    templateSheet = ref(""),
    required = ref("题名、作者、作者单位"),
    templateNotes = ref("");
  type LegacyPreview = {
    root: string;
    fingerprint: string;
    roster_count: number;
    journal_count: number;
    import_count: number;
    orphan_count: number;
    classification_count?: number;
    prepared_count?: number;
    material_file_count?: number;
    unbound_material_count?: number;
    material_warnings?: string[];
    conflicts?: {
      sa_id: string;
      kind: string;
      source: string;
      message: string;
    }[];
    entries: {
      sa_id: string;
      title: string;
      histories: number;
      materials?: number;
      phases: string[];
      input_changed: boolean;
      needs_readback: boolean;
    }[];
  };
  const legacyPreview = ref<LegacyPreview | null>(null);
  async function previewLegacy() {
    legacyPreview.value = null;
    const result = await run<LegacyPreview & { cancelled?: boolean }>(
      "preview_legacy",
      {},
      "已读取旧版记录，请核对迁移范围。",
    );
    if (result && !result.cancelled) legacyPreview.value = result;
  }
  async function migrateLegacy() {
    const preview = legacyPreview.value;
    if (!preview || preview.conflicts?.length) return;
    const result = await run(
      "migrate_legacy",
      { root: preview.root, fingerprint: preview.fingerprint },
      "旧版历史已迁移。已有平台操作请先回读，原目录未修改。",
    );
    if (result) legacyPreview.value = null;
  }
  const api = reactive({
    base: "https://models.sjtu.edu.cn/api/v1",
    model: "deepseek-chat",
    key: "",
    configured: false,
  });
  type Claim = {
    row: Record<string, unknown>;
    prepared: {
      item_id: string;
      staff_id: string;
      sa_text: string;
      person: { id: string; name: string; wno: string };
      authors: {
        index: number;
        fullname: string;
        order: string;
        eligible: boolean;
        scholarId: string;
      }[];
    };
    suggested_index: number | null;
  };
  const proof = ref(""),
    source = ref(""),
    claim = ref<Claim | null>(null),
    authorIndex = ref<number | null>(null);
  type AliasSnapshot = {
    staff_id: string;
    scholar: { id: string; wno: string; nameCn: string; nameEn: string };
    aliases: { id: string; nameAlias: string }[];
  };
  const aliases = ref<AliasSnapshot | null>(null),
    aliasName = ref(""),
    aliasEvidence = ref("");
  const duplicateScan = ref<DuplicateScan | null>(null),
    duplicatePrepared = ref<DuplicatePrepared | null>(null);
  const duplicateGroup = ref(""),
    mergeSource = ref(""),
    mergeTarget = ref(""),
    mergeEvidence = ref(""),
    mergeRetained = ref(""),
    mergeIdentity = ref(false);
  const mergeCandidates = computed(
    () =>
      duplicatePrepared.value?.group.items.filter((row) =>
        duplicatePrepared.value?.matched_ids.includes(row.id),
      ) || [],
  );
  const issueChecklist = ref<IssueChecklist | null>(null),
    issueKey = ref(""),
    issueOutcome = ref(""),
    issueSource = ref(""),
    issueProof = ref(""),
    issueNote = ref("");
  const selectedIssue = computed(() =>
    issueChecklist.value?.requirements.find((i) => i.key === issueKey.value),
  );
  const metadataPrepared = ref<MetadataPrepared | null>(null);
  const metadataAuthor = ref<number | null>(null);
  const metadataOperation = ref("role");
  const metadataOrder = ref<number[]>([]);
  const metadataOrderChanged = computed(
    () =>
      metadataOperation.value === "role" ||
      metadataOrder.value.some((index, position) => index !== position),
  );
  function clearMetadata() {
    metadataPrepared.value = null;
    metadataAuthor.value = null;
    metadataOperation.value = "role";
    metadataOrder.value = [];
  }
  const orderedMetadataRows = computed(() =>
    metadataOrder.value.map((index) => {
      const row =
        metadataOperation.value === "author_order"
          ? metadataPrepared.value?.result.authors.find(
              (r) => r.index === index,
            )
          : metadataPrepared.value?.result.institutions?.find(
              (r) => r.index === index,
            );
      return {
        index,
        label: row
          ? "fullname" in row
            ? row.fullname
            : row.address
          : "原列表条目",
      };
    }),
  );
  function chooseMetadataOperation() {
    metadataAuthor.value = null;
    const rows =
      metadataOperation.value === "author_order"
        ? metadataPrepared.value?.result.authors
        : metadataOperation.value === "institution_order"
          ? metadataPrepared.value?.result.institutions
          : [];
    metadataOrder.value = rows?.map((row) => row.index) || [];
  }
  function moveMetadataRow(position: number, offset: number) {
    if (
      locked.value ||
      position + offset < 0 ||
      position + offset >= metadataOrder.value.length
    )
      return;
    const next = [...metadataOrder.value];
    [next[position], next[position + offset]] = [
      next[position + offset],
      next[position],
    ];
    metadataOrder.value = next;
  }
  const libraryTitle = ref("");
  const librarySearch = computed<LibrarySearch | null>(() => {
    const evidence = current.value?.evidence
      .filter((e) => e.kind === "library_search")
      .slice(-1)[0];
    if (!evidence) return null;
    try {
      const receipt = JSON.parse(evidence.text);
      return receipt.result?.verified &&
        receipt.result.sa_id === current.value?.id
        ? receipt.result
        : null;
    } catch {
      return null;
    }
  });
  const libraryCheckReady = computed(() => {
    if (!["missing", "corrected_existing"].includes(review.route)) return true;
    const result = librarySearch.value;
    if (
      !result ||
      result.target.title !== libraryTitle.value.trim() ||
      result.target.doi !==
        (current.value?.artifact?.candidate.doi ||
          current.value?.record.doi ||
          "") ||
      result.target.wos !==
        (current.value?.artifact?.candidate.wos ||
          current.value?.record.wos ||
          "") ||
      Date.now() - result.checked_at > 24 * 60 * 60 * 1000
    )
      return false;
    if (
      review.route === "missing" &&
      !["pushed", "claimed"].includes(current.value?.stage || "")
    )
      return result.items.length === 0 && !review.platform_id;
    return result.items.some((i) => i.id === review.platform_id);
  });
  async function searchLibrary() {
    const id = current.value?.id;
    if (!id || !libraryTitle.value.trim()) return;
    const result = await run<LibrarySearch>(
      "run_step",
      {
        id,
        action: "library_search",
        approved: false,
        extra: { title: libraryTitle.value.trim() },
      },
      "本库查询的实际条件、分页和完整条目已保存。请逐条核对候选。",
    );
    if (result && selectedId.value === id) {
      review.platform_id = "";
      review.library_checked = libraryCheckReady.value;
    }
  }
  const editableAuthors = computed(() => {
    const field =
      issueKey.value === "corresponding_author"
        ? "correspondent"
        : issueKey.value === "first_author"
          ? "commonFirst"
          : "";
    return (
      metadataPrepared.value?.result.authors.filter(
        (a) =>
          a.eligible &&
          (metadataOperation.value === "role"
            ? a.fields.includes(field)
            : metadataOperation.value === "author_order"
              ? metadataPrepared.value?.result.can_reorder_authors === true
              : metadataOperation.value === "institution_order" &&
                metadataPrepared.value?.result.can_reorder_institutions ===
                  true),
      ) || []
    );
  });
  const issueOutcomes = computed(() => {
    const key = issueKey.value;
    if (
      ["corresponding_author", "first_author", "first_institution"].includes(
        key,
      )
    )
      return [
        { value: "sa_correct", label: "SA 正确，已修改本库并回读" },
        { value: "library_correct", label: "本库正确，保留本库值并备注" },
      ];
    if (key === "author_claim")
      return [{ value: "claimed", label: "已按完整工号回读认领关系" }];
    if (key === "identifiers")
      return [
        { value: "same_paper", label: "经 DOI 等原始来源核对，确认同一篇" },
      ];
    return key
      ? [{ value: "other_confirmed", label: "已核对这个原因，保存具体结论" }]
      : [];
  });
  const issueReady = computed(
    () =>
      !!current.value?.issue_plan &&
      current.value.issue_plan.requirements.every((i) =>
        current.value?.issue_reviews?.some((r) => r.key === i.key),
      ),
  );
  const requiresIssueChecklist = computed(() => {
    const task = current.value;
    const row = task?.sa_snapshot?.row as Record<string, unknown> | undefined;
    return (
      task?.record.matches === 1 ||
      !!task?.issue_plan ||
      Number(row?.matchCount) === 1
    );
  });
  const clearDuplicate = () => {
    duplicateScan.value = null;
    duplicatePrepared.value = null;
    duplicateGroup.value = "";
    mergeSource.value = "";
    mergeTarget.value = "";
    mergeEvidence.value = "";
    mergeRetained.value = "";
    mergeIdentity.value = false;
  };
  const review = reactive<Review>({
    route: "missing",
    evidence_id: "",
    library_checked: false,
    platform_id: "",
    affiliation_confirmed: false,
    identity_confirmed: false,
    issues_resolved: false,
    note: "",
  });
  const stageNames: Record<string, string> = {
    pending: "待处理",
    searching: "正在检索",
    downloading: "正在下载",
    downloaded: "已下载 · 待核验",
    ready: "材料已核验",
    uploaded: "上传成功",
    imported: "导入成功",
    pushed: "推送成功",
    claimed: "认领已核验",
    completed: "SA 已处理",
    awaiting_review: "待核验",
    unknown: "操作结果待确认",
  };
  const routeNames: Record<string, string> = {
    existing: "现有条目核对",
    duplicate: "重复条目核验",
    zero_review: "零匹配 · 原因待核对",
    non_sjtu: "非交大",
    corrected_existing: "纠正题名后本库已有",
    missing: "交大成果 · 本库缺失",
    not_found: "未查询到文献",
  };
  const owners = computed(() =>
    [...new Set(workspace.value.tasks.map((t) => t.record.owner))].sort(),
  );
  const scoped = computed(() =>
    workspace.value.tasks.filter(
      (t) => !owner.value || t.record.owner === owner.value,
    ),
  );
  const tasks = computed(() =>
    scoped.value.filter((t) => {
      const text = [
        t.record.title,
        t.id,
        t.record.doi,
        t.artifact?.candidate.title || "",
      ]
        .join(" ")
        .toLowerCase();
      return (
        text.includes(search.value.toLowerCase()) &&
        (filter.value === "all" ||
          (filter.value === "zero" && t.record.matches === 0) ||
          (filter.value === "review" &&
            (!!t.pending_input ||
              ["awaiting_review", "unknown", "downloaded"].includes(
                t.stage,
              ))) ||
          (filter.value === "done" &&
            t.stage === "completed" &&
            !t.pending_input) ||
          (filter.value === "skipped" && t.record.skipped))
      );
    }),
  );
  const current = computed(() =>
    workspace.value.tasks.find((t) => t.id === selectedId.value),
  );
  const versionPrepared = ref<{
    proposal_id: string;
    snapshot: Record<string, unknown>;
  } | null>(null);
  const versionSource = ref(""),
    versionProof = ref(""),
    versionConfirmed = ref(false);
  const versionFields = [
    ["owner", "负责人"],
    ["title", "题名"],
    ["doi", "DOI"],
    ["wos", "WOS ID"],
    ["staff_id", "完整工号"],
    ["matches", "匹配数"],
    ["item_ids", "平台唯一号"],
    ["mark", "平台标记"],
    ["reason", "待处理原因"],
    ["row", "原表行号"],
  ] as const;
  const versionDiffs = computed(() =>
    versionFields
      .filter(
        ([key]) =>
          current.value?.pending_input &&
          current.value.record[key] !== current.value.pending_input.record[key],
      )
      .map(([key, label]) => ({
        label,
        before: String(current.value!.record[key]),
        after: String(current.value!.pending_input!.record[key]),
      })),
  );
  watch(
    [() => selectedId.value, () => current.value?.pending_input?.id],
    () => {
      versionPrepared.value = null;
      versionSource.value = "";
      versionProof.value = "";
      versionConfirmed.value = false;
    },
  );
  async function prepareVersion() {
    const t = current.value;
    if (!t?.pending_input) return;
    const proposalId = t.pending_input.id;
    versionPrepared.value = null;
    versionConfirmed.value = false;
    const result = await run<{
      proposal_id: string;
      snapshot: Record<string, unknown>;
    }>(
      "prepare_input_version",
      { id: t.id },
      "新名单已与实时 SA 核对，请确认版本与责任人安排。",
    );
    if (
      result &&
      current.value?.id === t.id &&
      current.value.pending_input?.id === proposalId
    )
      versionPrepared.value = result;
  }
  async function acceptVersion() {
    const t = current.value,
      p = versionPrepared.value;
    if (
      !t?.pending_input ||
      !p ||
      p.proposal_id !== t.pending_input.id ||
      !versionConfirmed.value
    )
      return;
    const result = await run<Task>(
      "accept_input_version",
      {
        id: t.id,
        proposalId: p.proposal_id,
        source: versionSource.value,
        proof: versionProof.value,
        approved: true,
      },
      "已采用核验后的新名单版本；旧历史保留，业务结论需重新核对。",
    );
    if (result && selectedId.value === t.id) {
      selectedId.value = "";
      owner.value = result.record.owner;
      await nextTick();
      selectedId.value = t.id;
    } else if (!result && selectedId.value === t.id) {
      versionPrepared.value = null;
      versionConfirmed.value = false;
    }
  }
  const counts = computed(() => ({
    all: scoped.value.length,
    zero: scoped.value.filter((t) => t.record.matches === 0).length,
    review: scoped.value.filter(
      (t) =>
        !!t.pending_input ||
        ["awaiting_review", "unknown", "downloaded"].includes(t.stage),
    ).length,
    done: scoped.value.filter(
      (t) => t.stage === "completed" && !t.pending_input,
    ).length,
  }));
  const locked = computed(() => pending.value || workspace.value.running);
  watch(selectedId, () => {
    const t = current.value;
    claim.value = null;
    aliases.value = null;
    aliasName.value = "";
    aliasEvidence.value = "";
    clearDuplicate();
    issueChecklist.value = null;
    issueKey.value = "";
    issueOutcome.value = "";
    issueSource.value = "";
    issueProof.value = "";
    issueNote.value = "";
    clearMetadata();
    libraryTitle.value = t?.artifact?.candidate.title || t?.record.title || "";
    proof.value = "";
    source.value = t?.artifact?.record_url || "";
    Object.assign(
      review,
      t?.review || {
        route:
          t?.record.matches === 1
            ? "existing"
            : (t?.record.matches || 0) >= 2
              ? "duplicate"
              : "missing",
        evidence_id: "",
        library_checked: false,
        platform_id: t?.platform_id || "",
        affiliation_confirmed: false,
        identity_confirmed: false,
        issues_resolved: false,
        note: "",
      },
    );
  });
  watch(
    () => current.value?.platform_id,
    (id, previous) => {
      if (id && (!review.platform_id || review.platform_id === previous))
        review.platform_id = id;
    },
  );
  watch(
    () => current.value?.artifact?.candidate.title,
    (title, previous) => {
      if (
        title &&
        (!libraryTitle.value ||
          libraryTitle.value === previous ||
          libraryTitle.value === current.value?.record.title)
      )
        libraryTitle.value = title;
    },
  );
  watch(duplicateGroup, () => {
    duplicatePrepared.value = null;
    mergeSource.value = "";
    mergeTarget.value = "";
    mergeIdentity.value = false;
  });
  watch(issueKey, () => {
    issueOutcome.value = "";
    issueSource.value = "";
    issueProof.value = "";
    issueNote.value = "";
    clearMetadata();
  });
  watch(
    () =>
      [
        issueReady.value,
        requiresIssueChecklist.value,
        selectedId.value,
      ] as const,
    ([ready, required]) => {
      if (required) review.issues_resolved = ready;
    },
  );
  watch(libraryCheckReady, (value) => {
    if (
      current.value?.record.matches === 0 &&
      ["missing", "corrected_existing"].includes(review.route)
    )
      review.library_checked = value;
  });
  watch(
    () => review.route,
    (route) => {
      if (route === "not_found") {
        review.platform_id = "";
        review.identity_confirmed = false;
        review.affiliation_confirmed = false;
        review.library_checked = false;
        review.issues_resolved = false;
        if (!review.note.trim())
          review.note = "未查询到该文献（实际检索范围见检索记录）";
      }
    },
  );
  let refreshInFlight: Promise<void> | null = null;
  let refreshAgain = false;
  let refreshTimer: ReturnType<typeof setTimeout> | null = null;
  let disposed = false;
  function refresh(): Promise<void> {
    if (disposed) return Promise.resolve();
    // Only one IPC snapshot can run at a time. Events received while it runs
    // request a follow-up snapshot instead of racing older responses into Vue.
    refreshAgain = true;
    if (refreshInFlight) return refreshInFlight;
    refreshInFlight = (async () => {
      do {
        refreshAgain = false;
        const snapshot = await callDesktop<Workspace>("workspace");
        if (!disposed) workspace.value = snapshot;
      } while (refreshAgain && !disposed);
    })().finally(() => {
      refreshInFlight = null;
    });
    return refreshInFlight;
  }
  function scheduleRefresh() {
    if (disposed || refreshTimer !== null) return;
    refreshTimer = setTimeout(() => {
      refreshTimer = null;
      void refresh().catch(() => {});
    }, 180);
  }
  function describe(e: unknown) {
    if (e && typeof e === "object" && "message" in e)
      return String((e as { message: unknown }).message);
    return String(e);
  }
  async function run<T>(
    command: DesktopCommand,
    args: Record<string, unknown> = {},
    success = "操作已完成。",
  ): Promise<T | undefined> {
    const pausing = command === "pause_queue" || command === "pause_ai_queue";
    const parallelControl =
      pausing ||
      [
        "source_browser_state",
        "focus_source_browser",
        "close_source_browser",
      ].includes(command);
    if (pending.value && !parallelControl) return;
    if (!parallelControl) pending.value = true;
    error.value = false;
    message.value = pausing ? "正在请求暂停…" : "正在执行，请保持工作页打开…";
    try {
      const result = await callDesktop<T>(command, args);
      message.value =
        result &&
        typeof result === "object" &&
        "cancelled" in result &&
        result.cancelled
          ? "已取消。"
          : success;
      return result;
    } catch (e) {
      error.value = true;
      message.value = describe(e);
    } finally {
      if (!parallelControl) pending.value = false;
      await refresh().catch(() => {});
    }
  }
  async function queue(retry = false) {
    await run(
      "run_queue",
      { owner: owner.value, retrySkipped: retry },
      "本轮队列已结束。逐条结果已保存；请查看待核验项和未执行任务。",
    );
  }
  const queuePending = computed(
    () =>
      !!workspace.value.download_queue &&
      !["completed", "cancelled"].includes(
        workspace.value.download_queue.status,
      ),
  );
  const queueStatusNames: Record<string, string> = {
    running: "执行中",
    paused: "已暂停",
    blocked: "工作页待恢复",
    interrupted: "重启后待继续",
    completed: "已结束",
    cancelled: "已结束原范围",
  };
  async function resumeQueue() {
    const q = workspace.value.download_queue;
    if (!q || !queuePending.value) return;
    await run(
      "resume_queue",
      { id: q.id },
      "原队列进度已保存，请查看逐篇结果和剩余项。",
    );
  }
  async function cancelQueue() {
    const q = workspace.value.download_queue;
    if (
      !q ||
      !queuePending.value ||
      !(await confirmAction(
        `结束 ${q.owner} 的原下载范围？已保存的文件和结果会保留，剩余 ${q.targets.length - q.cursor} 项不再执行。`,
        "结束原下载范围",
      ))
    )
      return;
    await run(
      "cancel_queue",
      { id: q.id },
      "原下载范围已结束，已保存的文件和结果仍保留。可以选择新范围。",
    );
  }
  async function step(action: string, write = false) {
    const t = current.value;
    if (!t) return;
    const extra =
      action === "submit_claim"
        ? { author_index: authorIndex.value }
        : action === "add_alias"
          ? { alias: aliasName.value, evidence_id: aliasEvidence.value }
          : action === "merge_duplicate"
            ? {
                source_id: mergeSource.value,
                target_id: mergeTarget.value,
                evidence_id: mergeEvidence.value,
                retained: mergeRetained.value,
                identity_confirmed: mergeIdentity.value,
              }
            : {};
    const labels: Record<string, string> = {
      import_upload: "上传核验通过的原始文件",
      import_submit: "提交导入",
      import_push: "按 PPT 五项设置推送",
      link: "关联核验后的平台唯一号",
      complete: "设置 SA 已处理并保存核验备注",
      submit_claim: "提交已核验的作者认领",
      add_alias: "保存来源已证实的学者别名",
      merge_duplicate: "将选定条目合并至主条目",
    };
    if (
      write &&
      !(await confirmAction(
        `${labels[action] || action}\n\n题名：${t.record.title}\nSA ID：${t.id}\n${action === "import_push" ? workspace.value.policy : action === "add_alias" ? `工号：${aliases.value?.staff_id}\n学者：${aliases.value?.scholar.nameCn || aliases.value?.scholar.nameEn}\n别名：${aliasName.value}` : action === "merge_duplicate" ? `被合并 ID：${extra.source_id}\n主条目 ID：${extra.target_id}\n保留字段核对：${extra.retained}` : ""}\n确认执行本条操作？`,
      ))
    )
      return;
    if (selectedId.value !== t.id) {
      error.value = true;
      message.value = "当前任务已切换，请重新核对操作对象。";
      return;
    }
    const result = await run<Claim>(
      "run_step",
      {
        id: t.id,
        action,
        approved: write,
        extra,
      },
      write
        ? "操作返回。请查看当前阶段，必要时继续回读平台结果。"
        : "工作页读取完成。",
    );
    if (action === "prepare_claim" && result && selectedId.value === t.id) {
      claim.value = result;
      authorIndex.value = null;
    }
    if (
      ["add_alias", "verify_alias"].includes(action) &&
      result &&
      selectedId.value === t.id
    ) {
      aliases.value = null;
      aliasName.value = "";
      aliasEvidence.value = "";
    }
    if (
      ["merge_duplicate", "verify_duplicate"].includes(action) &&
      result &&
      selectedId.value === t.id
    )
      clearDuplicate();
    if (action === "verify_metadata" && result && selectedId.value === t.id) {
      clearMetadata();
      await prepareIssues();
    }
  }
  async function scanDuplicates() {
    const id = current.value?.id;
    if (!id) return;
    const result = await run<DuplicateScan>(
      "run_step",
      { id, action: "scan_duplicates", approved: false, extra: {} },
      "重复候选已读取，请选择需要核对的候选组。",
    );
    if (result && selectedId.value === id) {
      clearDuplicate();
      duplicateScan.value = result;
    }
  }
  async function prepareDuplicate() {
    const id = current.value?.id,
      group = duplicateGroup.value;
    if (!id || !group) return;
    const result = await run<DuplicatePrepared>(
      "run_step",
      {
        id,
        action: "prepare_duplicate",
        approved: false,
        extra: { group_id: group },
      },
      "候选字段已读取，请逐项核对并明确两个条目。",
    );
    if (result && selectedId.value === id && duplicateGroup.value === group)
      duplicatePrepared.value = result;
  }
  async function prepareAlias() {
    const id = current.value?.id;
    if (!id) return;
    const result = await run<AliasSnapshot>(
      "run_step",
      {
        id,
        action: "prepare_alias",
        approved: false,
        extra: {},
      },
      "已按工号读取学者与现有别名。",
    );
    if (result && selectedId.value === id) aliases.value = result;
  }
  async function saveReview() {
    if (current.value?.record.matches === 0 && !libraryCheckReady.value) {
      error.value = true;
      message.value = "先用正确题名查询本库，再核对候选条目或明确零结果。";
      return;
    }
    if (!current.value) return;
    await run(
      "review_task",
      {
        id: current.value.id,
        review: { ...review },
        source: source.value,
        proof: proof.value,
      },
      "核验结论与来源已保存。",
    );
  }
  async function prepareIssues() {
    const id = current.value?.id;
    if (!id) return;
    const result = await run<IssueChecklist>(
      "run_step",
      { id, action: "prepare_issues", approved: false, extra: {} },
      "已读取每个待处理原因及实时字段。",
    );
    if (result && selectedId.value === id) issueChecklist.value = result;
  }
  async function prepareMetadata() {
    const id = current.value?.id,
      key = issueKey.value;
    if (
      !id ||
      !["corresponding_author", "first_author", "first_institution"].includes(
        key,
      )
    )
      return;
    const result = await run<MetadataPrepared>(
      "run_step",
      { id, action: "prepare_metadata", approved: false, extra: {} },
      "已按完整工号读取学者身份与实际作者控件，请明确选择作者行。",
    );
    if (result && selectedId.value === id && issueKey.value === key) {
      metadataPrepared.value = result;
      metadataAuthor.value = null;
      metadataOperation.value =
        key === "first_institution" ? "institution_order" : "role";
      chooseMetadataOperation();
    }
  }
  async function saveMetadata() {
    const id = current.value?.id,
      prepared = metadataPrepared.value,
      key = issueKey.value;
    const author = editableAuthors.value.find(
      (a) => a.index === metadataAuthor.value,
    );
    if (
      !id ||
      !prepared ||
      !author ||
      !metadataOrderChanged.value ||
      issueOutcome.value !== "sa_correct" ||
      !issueSource.value.trim() ||
      !issueProof.value.trim() ||
      !issueNote.value.trim()
    )
      return;
    const extra = {
      key,
      operation: metadataOperation.value,
      order: [...metadataOrder.value],
      author_index: author.index,
      source: issueSource.value,
      proof: issueProof.value,
      note: issueNote.value,
    };
    if (
      !(await confirmAction(
        `修改本库字段并保存\n\nSA ID：${id}\n平台条目：${prepared.result.item_id}\n完整工号：${prepared.result.staff_id}\n作者署名：${author.fullname}\n作者 ID：${author.id}\n字段：${selectedIssue.value?.label}\n方式：${extra.operation === "author_order" ? "调整完整作者署名顺序" : extra.operation === "institution_order" ? "调整完整单位顺序并同步作者单位编号" : key === "first_author" ? "共同第一作者标记" : "通讯作者标记"}\n按已核对的 SA 值：${selectedIssue.value?.sa}\n${extra.operation !== "role" ? "新顺序：\n" + orderedMetadataRows.value.map((row, i) => `${i + 1}. ${row.label}`).join("\n") + "\n" : ""}原文来源：${extra.source}\n具体依据：${extra.proof}\n\n确认保存本条修改，并回读完整本库字段和 SA？`,
      ))
    )
      return;
    if (
      selectedId.value !== id ||
      issueKey.value !== key ||
      metadataPrepared.value !== prepared ||
      metadataAuthor.value !== author.index ||
      metadataOperation.value !== extra.operation ||
      JSON.stringify(metadataOrder.value) !== JSON.stringify(extra.order) ||
      issueOutcome.value !== "sa_correct" ||
      extra.source !== issueSource.value ||
      extra.proof !== issueProof.value ||
      extra.note !== issueNote.value
    ) {
      error.value = true;
      message.value = "编辑对象或依据已变化，请重新核对。";
      return;
    }
    const result = await run(
      "run_step",
      { id, action: "save_metadata", approved: true, extra },
      "本库修改已保存；完整字段和 SA 已回读，本项结论已记录。",
    );
    if (result && selectedId.value === id) {
      clearMetadata();
      await prepareIssues();
    }
  }
  async function saveIssue() {
    const id = current.value?.id;
    if (!id || !issueChecklist.value) return;
    const result = await run(
      "run_step",
      {
        id,
        action: "review_issue",
        approved: false,
        extra: {
          key: issueKey.value,
          outcome: issueOutcome.value,
          source: issueSource.value,
          proof: issueProof.value,
          note: issueNote.value,
          expected: JSON.parse(JSON.stringify(issueChecklist.value.live)),
        },
      },
      "本项来源、结论和实时回读已保存。",
    );
    if (result && selectedId.value === id) {
      issueSource.value = "";
      issueProof.value = "";
      issueNote.value = "";
    }
  }
  async function saveApi() {
    await run(
      "save_ai_settings",
      { base: api.base, model: api.model, key: api.key },
      "API 设置已保存，密钥存入系统凭据。",
    );
    api.key = "";
    Object.assign(api, await callDesktop("ai_settings"));
  }
  async function register() {
    await run(
      "register_template",
      {
        sheet: templateSheet.value,
        headerRow: Math.max(0, templateRow.value - 1),
        required: required.value
          .split(/[、,，\n]/)
          .map((s) => s.trim())
          .filter(Boolean),
        notes: templateNotes.value,
      },
      "模板已注册，原始格式与工作表已保留。",
    );
    schemas.value = await callDesktop("templates");
  }
  async function classify() {
    if (!current.value) return;
    await run(
      "classify_task",
      {
        id: current.value.id,
        template: schemas.value.find((t) => t.id === templateId.value) || null,
      },
      "AI 建议已保存。字段及结论需结合来源核对。",
    );
  }
  async function fill() {
    if (!current.value) return;
    const target = {
      id: current.value.id,
      revision: current.value.revision,
      template: templateId.value,
    };
    materialValidation.value = null;
    const result = await run<MaterialValidation>("fill_template", {
      id: current.value.id,
      templateId: templateId.value,
    });
    if (result && !result.cancelled) {
      if (
        current.value?.id !== target.id ||
        current.value.revision !== (result.task_revision ?? target.revision) ||
        templateId.value !== target.template
      )
        return;
      materialValidation.value = result;
      const problems = [
        ...result.missing,
        ...(result.invalid || []).map((p) => p.column),
        ...(result.requires_review || []).map((p) => p.column),
      ];
      message.value =
        result.ready === true && !problems.length
          ? `材料已保存，模板校验通过：${result.path}`
          : `材料草稿已保存，待补充或核对：${[...new Set(problems)].join("、") || "模板规则"}`;
    }
  }
  const materialValidation = ref<MaterialValidation | null>(null);
  watch([selectedId, templateId, () => current.value?.revision], () => {
    materialValidation.value = null;
  });
  const unlisteners: UnlistenFn[] = [];
  onMounted(async () => {
    try {
      await refresh();
      Object.assign(api, await callDesktop("ai_settings"));
      schemas.value = await callDesktop("templates");
      unlisteners.push(await listen("workspace-changed", scheduleRefresh));
      unlisteners.push(
        await listen<{
          task_id: string;
          phase: string;
          elapsed_seconds: number;
        }>("wos-progress", (e) => {
          if (!pending.value || workspace.value.paused || error.value) return;
          const phases: Record<string, string> = {
            preparing: "正在准备 WOS 检索页",
            waiting_results: "等待 WOS 返回本次检索结果",
            opening_record: "正在打开并核对目标文献",
            preparing_export: "正在选择完整记录导出",
            waiting_download: "等待 WOS 文件下载完成",
          };
          const phase = phases[e.payload.phase];
          if (phase)
            message.value = `${e.payload.task_id} · ${phase}（${e.payload.elapsed_seconds} 秒）`;
        }),
      );
      unlisteners.push(
        await listen<{ success: boolean }>("download-event", (e) => {
          message.value = e.payload.success
            ? "文件下载完成，正在等待身份核验。"
            : "文件下载未完成，请查看任务原因。";
        }),
      );
      unlisteners.push(
        await listen("source-download-started", () => {
          error.value = false;
          message.value =
            "正在下载原始来源。下载结束并保存回执后，才能开始其他操作；关闭下载窗口会保留为未确认。";
          scheduleRefresh();
        }),
      );
      unlisteners.push(
        await listen<{ success: boolean; error?: Failure }>(
          "source-download-event",
          (e) => {
            error.value = !e.payload.success;
            message.value = e.payload.success
              ? "来源文件已保存。请刷新原始下载记录，选择论文并确认绑定。"
              : e.payload.error?.message ||
                "原始来源下载未确认完成，请查看下载记录。";
            scheduleRefresh();
          },
        ),
      );
    } catch (e) {
      error.value = true;
      message.value = describe(e);
    }
  });
  onUnmounted(() => {
    disposed = true;
    if (refreshTimer !== null) clearTimeout(refreshTimer);
    unlisteners.forEach((fn) => fn());
  });

  const confirmation = ref<{
    description: string;
    title: string;
    resolve: (answer: boolean) => void;
  } | null>(null);
  function confirmAction(description: string, title = "核对本条平台操作") {
    confirmation.value?.resolve(false);
    return new Promise<boolean>(
      (resolve) => (confirmation.value = { description, title, resolve }),
    );
  }
  function settleConfirmation(answer: boolean) {
    const current = confirmation.value;
    confirmation.value = null;
    current?.resolve(answer);
  }
  return {
    workspace,
    page,
    owner,
    search,
    filter,
    selectedId,
    message,
    error,
    pending,
    detailTab,
    schemas,
    legacyPreview,
    previewLegacy,
    migrateLegacy,
    templateId,
    templateRow,
    templateSheet,
    required,
    templateNotes,
    materialValidation,
    api,
    proof,
    source,
    claim,
    authorIndex,
    aliases,
    aliasName,
    aliasEvidence,
    prepareAlias,
    duplicateScan,
    duplicatePrepared,
    duplicateGroup,
    mergeSource,
    mergeTarget,
    mergeEvidence,
    mergeRetained,
    mergeIdentity,
    mergeCandidates,
    scanDuplicates,
    prepareDuplicate,
    review,
    stageNames,
    routeNames,
    owners,
    scoped,
    tasks,
    current,
    versionPrepared,
    versionSource,
    versionProof,
    versionConfirmed,
    versionDiffs,
    prepareVersion,
    acceptVersion,
    counts,
    locked,
    refresh,
    run,
    queue,
    queuePending,
    queueStatusNames,
    resumeQueue,
    cancelQueue,
    step,
    saveReview,
    issueChecklist,
    issueKey,
    issueOutcome,
    issueSource,
    issueProof,
    issueNote,
    selectedIssue,
    issueOutcomes,
    issueReady,
    requiresIssueChecklist,
    prepareIssues,
    saveIssue,
    metadataPrepared,
    metadataAuthor,
    metadataOperation,
    metadataOrder,
    metadataOrderChanged,
    orderedMetadataRows,
    chooseMetadataOperation,
    moveMetadataRow,
    libraryTitle,
    librarySearch,
    libraryCheckReady,
    searchLibrary,
    editableAuthors,
    prepareMetadata,
    saveMetadata,
    saveApi,
    register,
    classify,
    fill,
    confirmation,
    settleConfirmation,
  };
}
