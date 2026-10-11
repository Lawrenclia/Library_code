<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Task,
  SourceBrowserState,
  SourceDownload,
  SourcePage,
  SourceWindowState,
  SourceFlow,
} from "@/types";
import { callDesktop, type DesktopCommand } from "@/services/desktop";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
const props = defineProps<{
  task: Task;
  channel: string;
  locked: boolean;
  encoding: string;
  delimiter: string;
  textTable: boolean;
  run: <T>(
    command: DesktopCommand,
    args?: Record<string, unknown>,
    success?: string,
  ) => Promise<T | undefined>;
}>();
const emit = defineEmits<{ preview: [page: SourcePage, url: string] }>();
const state = ref<SourceBrowserState | null>(null),
  entry = ref(""),
  origins = ref("");
const loading = ref(false),
  windowAction = ref("");
const acquisitionProgress = ref("");
const flowText = ref(""),
  flowError = ref("");
const exportReviewNote = ref("");
const selectedExportReceipt = ref<Record<string, string>>({});
const defaultFlow = (): SourceFlow => ({
  channel: props.channel,
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
});
const savedFlow = computed(
  () =>
    state.value?.flows?.find((f) => f.channel === props.channel) ||
    defaultFlow(),
);
const flowDirty = computed(() => {
  try {
    return (
      JSON.stringify(JSON.parse(flowText.value || "{}")) !==
      JSON.stringify(savedFlow.value)
    );
  } catch {
    return true;
  }
});
const acquisitions = computed(
  () =>
    state.value?.acquisitions?.filter(
      (a) => a.session.site.channel === props.channel,
    ) || [],
);
const sourceQueue = computed(() => state.value?.source_queue);
const lastQueueResult = computed(
  () => sourceQueue.value?.results[sourceQueue.value.results.length - 1],
);
const queueWorking = ref(false),
  queueError = ref("");
let queued = false,
  resetQueued = false,
  disposed = false;
const windows = computed(() => state.value?.windows || []);
const unlisteners: UnlistenFn[] = [];
onMounted(() => {
  void listen<{ id: string; channel: string; message: string }>(
    "source-acquisition-progress",
    ({ payload }) => {
      if (
        !disposed &&
        payload.id === props.task.id &&
        payload.channel === props.channel &&
        windowAction.value
      )
        acquisitionProgress.value = payload.message;
    },
  )
    .then((unlisten) => {
      if (disposed) unlisten();
      else unlisteners.push(unlisten);
    })
    .catch(() => {});
  for (const event of [
    "source-windows-changed",
    "source-download-started",
    "source-download-event",
    "source-queue-changed",
  ]) {
    void listen(event, () => {
      if (state.value && !disposed) void load(false, true);
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlisteners.push(unlisten);
      })
      .catch(() => {});
  }
});
onUnmounted(() => {
  disposed = true;
  unlisteners.forEach((unlisten) => unlisten());
});
const downloads = computed(
  () =>
    state.value?.downloads.filter(
      (d) => d.session.site.channel === props.channel,
    ) || [],
);
const savedSite = computed(() =>
  state.value?.sites.find((s) => s.channel === props.channel),
);
const changedSite = computed(
  () =>
    entry.value.trim() !== (savedSite.value?.entry_url || "") ||
    origins.value.trim() !==
      (savedSite.value?.download_origins.join("\n") || ""),
);
const labels: Record<string, string> = {
  requested: "下载中",
  completed: "文件下载完成，待选择记录",
  failed: "下载失败",
  interrupted: "下载结束未确认",
};
watch(
  () => [props.task.id, props.channel],
  () => {
    state.value = null;
    entry.value = "";
    origins.value = "";
    acquisitionProgress.value = "";
  },
);
async function load(resetConfig = true, quiet = false) {
  if (loading.value) {
    queued = true;
    resetQueued ||= resetConfig;
    return;
  }
  loading.value = true;
  const id = props.task.id,
    channel = props.channel;
  try {
    const result = quiet
      ? await callDesktop<SourceBrowserState>("source_browser_state", {
          id,
        }).catch(() => undefined)
      : await props.run<SourceBrowserState>(
          "source_browser_state",
          { id },
          "已读取来源入口与原始下载记录。",
        );
    if (
      !result ||
      disposed ||
      props.task.id !== id ||
      props.channel !== channel
    )
      return;
    state.value = result;
    if (resetConfig) {
      const site = result.sites.find((s) => s.channel === channel);
      entry.value = site?.entry_url || "";
      origins.value = site?.download_origins.join("\n") || "";
      flowText.value = JSON.stringify(
        result.flows?.find((f) => f.channel === channel) || defaultFlow(),
        null,
        2,
      );
      flowError.value = "";
    }
  } finally {
    loading.value = false;
    if (queued && !disposed) {
      const reset = resetQueued;
      queued = false;
      resetQueued = false;
      void load(reset, true);
    }
  }
}
async function saveFlow() {
  flowError.value = "";
  try {
    const flow = JSON.parse(flowText.value) as SourceFlow;
    flow.channel = props.channel;
    const result = await props.run(
      "save_source_flow",
      { flow },
      "已保存渠道流程，后续执行会核对实际页面文字。",
    );
    if (result) await load();
  } catch {
    flowError.value = "流程配置不是完整 JSON，请检查后保存。";
  }
}
async function acquire(
  action: "auto" | "capture" | "export",
  window?: SourceWindowState,
) {
  if (windowAction.value || props.locked) return;
  const id = props.task.id,
    channel = props.channel;
  windowAction.value = window?.label || "source-acquisition";
  acquisitionProgress.value = "正在获取渠道来源…";
  try {
    const page = await props.run<SourcePage>(
      "acquire_source",
      { id, channel, action, label: window?.label || null },
      "来源已保存；请核对原始文件或详情来源及重要缺项。",
    );
    if (
      !disposed &&
      props.task.id === id &&
      props.channel === channel &&
      page?.draft &&
      page.draft.task_revision === props.task.revision
    )
      emit("preview", page, page.acquisition?.source_url || window?.url || "");
    await load(false, true);
  } finally {
    windowAction.value = "";
    acquisitionProgress.value = "";
  }
}
async function recheck(operationId: string) {
  await props.run(
    "recheck_source_export",
    {
      id: props.task.id,
      operationId,
      downloadId: selectedExportReceipt.value[operationId] || null,
    },
    "已检查原下载回执；结果未知时不会重新导出。",
  );
  await load(false, true);
}
async function previewCapture(operationId: string) {
  const id = props.task.id;
  const page = await props.run<SourcePage>(
    "preview_source_capture",
    { id, operationId },
    "已重新读取保存的详情来源；原文件字节已核对。",
  );
  if (
    page &&
    props.task.id === id &&
    page.draft.task_revision === props.task.revision
  )
    emit("preview", page, page.acquisition?.source_url || "");
}
async function cancelExport(operationId: string) {
  const result = await props.run(
    "cancel_source_export",
    { id: props.task.id, operationId, note: exportReviewNote.value },
    "原导出记录已取消，文件与历史回执保留。",
  );
  if (result) {
    exportReviewNote.value = "";
    await load(false, true);
  }
}
async function queueControl(action: "start" | "pause" | "resume" | "cancel") {
  if (queueWorking.value && action !== "pause") return;
  queueError.value = "";
  if (action !== "pause") queueWorking.value = true;
  try {
    await callDesktop("source_queue_control", {
      action,
      owner: props.task.record.owner,
      channel: props.channel,
    });
    await load(false, true);
  } catch (e) {
    queueError.value = (e as { message?: string }).message || String(e);
  } finally {
    if (action !== "pause") queueWorking.value = false;
  }
}
async function manage(window: SourceWindowState, close: boolean) {
  if (windowAction.value) return;
  const id = props.task.id;
  windowAction.value = window.label;
  try {
    await props.run(
      close ? "close_source_browser" : "focus_source_browser",
      { id, label: window.label },
      close
        ? "已请求关闭本篇来源窗口；未确认下载保留原文件与请求。"
        : "已切回本篇来源窗口；请核对实际网页与登录状态。",
    );
    if (props.task.id === id) await load(false, true);
  } finally {
    windowAction.value = "";
  }
}
async function save() {
  const id = props.task.id,
    channel = props.channel;
  const result = await props.run(
    "save_source_site",
    {
      site: {
        channel,
        entry_url: entry.value.trim(),
        download_origins: origins.value
          .split(/\r?\n/)
          .map((s) => s.trim())
          .filter(Boolean),
      },
    },
    "数据库入口已保存；可在本篇窗口获取来源或执行原始导出。",
  );
  if (result && props.task.id === id && props.channel === channel) await load();
}
async function open() {
  await props.run(
    "open_source_browser",
    { id: props.task.id, channel: props.channel },
    "已打开或切回本篇来源窗口。请先完成机构登录；下载由应用保存。",
  );
  await load(false, true);
}
async function cnki(
  action: "search" | "capture" | "auto",
  window?: SourceWindowState,
) {
  if (windowAction.value || props.locked) return;
  const { id, revision } = props.task;
  windowAction.value = window?.label || "cnki-search";
  acquisitionProgress.value =
    action === "auto" ? "正在检索并获取完整题录…" : "正在打开来源或获取题录…";
  try {
    const page = await props.run<SourcePage>(
      "cnki_source",
      { id, action, label: window?.label || null },
      action === "search"
        ? "已按名单题名打开 CNKI。请完成登录，核对并打开目标论文详情。"
        : "已保存完整题录与原始响应，请选择记录并核对身份和缺项。",
    );
    if (props.task.id !== id || props.task.revision !== revision || disposed)
      return;
    if (
      action !== "search" &&
      page?.draft &&
      page.draft.task_revision === revision
    )
      emit("preview", page, page.acquisition?.source_url || "");
    await load(false, true);
  } finally {
    windowAction.value = "";
    acquisitionProgress.value = "";
  }
}
async function preview(download: SourceDownload, resave = false) {
  const id = props.task.id,
    channel = props.channel;
  const page = await props.run<SourcePage>(
    "preview_source_download",
    {
      id,
      downloadId: download.id,
      resave,
      options: {
        encoding: props.encoding,
        delimiter: props.delimiter,
        text_table: props.textTable,
      },
    },
    "原始下载已读取，请明确选择论文记录并填写绑定依据。",
  );
  if (
    page &&
    props.task.id === id &&
    props.channel === channel &&
    page.draft.task_revision === props.task.revision
  )
    emit("preview", page, download.page_url);
}
</script>
<template>
  <section class="space-y-2 rounded-lg border p-3" aria-label="内置来源浏览器">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <p class="text-xs font-medium">数据库原始下载</p>
      <Button size="sm" variant="outline" :disabled="loading" @click="load()"
        >读取入口与下载记录</Button
      >
    </div>
    <p class="text-[11px] leading-6 text-muted-foreground">
      登录数据库后，可以自动获取本篇来源，也可从详情页下载原文件。应用记录来源与缺项；论文身份、交大归属和本库是否缺失仍需分别核验。
    </p>
    <div v-if="channel === 'cnki'" class="space-y-2 rounded-md bg-muted/40 p-2">
      <Button
        size="sm"
        :disabled="locked || !!windowAction"
        @click="cnki('auto')"
      >
        {{ windowAction ? "正在获取…" : "自动检索并获取 CNKI 题录" }}
      </Button>
      <Button
        size="sm"
        variant="outline"
        :disabled="locked || !!windowAction"
        @click="cnki('search')"
      >
        按名单题名打开 CNKI
      </Button>
      <p class="text-[11px] leading-5 text-muted-foreground">
        自动获取先检查完整结果，再进入唯一精确题名的详情页，保存完整题录、原始响应和来源。需登录、验证码或结果多页时，请在窗口处理后继续；也可打开核对后的详情，点击下方“获取题录”。
        题录用于 AI 整理和模板填表。若网页可导出
        Excel/TXT，请优先下载原文件；文件直接保存到工作目录。
      </p>
    </div>
    <p
      v-if="acquisitionProgress"
      class="text-xs"
      role="status"
      aria-live="polite"
    >
      {{ acquisitionProgress }}
    </p>
    <template v-if="state">
      <label class="field"
        >数据库入口<Input
          v-model="entry"
          :disabled="locked"
          placeholder="填写实际数据库网页地址"
      /></label>
      <details>
        <summary class="cursor-pointer text-[11px] text-muted-foreground">
          其他下载来源域名
        </summary>
        <Textarea
          v-model="origins"
          :disabled="locked"
          placeholder="每行一个实际下载来源，如 https://example.org；入口域名自动保留"
        />
        <p class="text-[10px] text-muted-foreground">
          只登记实际使用的页面、代理或导出文件来源。未登记的下载域名会被拒绝。
        </p>
      </details>
      <div class="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          :disabled="locked || !entry.trim()"
          @click="save"
          >保存入口</Button
        >
        <Button
          size="sm"
          :disabled="locked || !savedSite || changedSite"
          @click="open"
          >打开或切回本篇来源窗口</Button
        >
      </div>
      <p v-if="changedSite" class="text-[11px] text-muted-foreground">
        先保存入口与下载来源，再打开窗口。
      </p>
      <div class="space-y-2 rounded-md bg-muted/40 p-2">
        <Button
          size="sm"
          :disabled="
            locked || !!windowAction || !savedSite || changedSite || flowDirty
          "
          @click="acquire('auto')"
          >检索并获取原始来源</Button
        >
        <p class="text-[11px] leading-5 text-muted-foreground">
          先核对完整结果与唯一精确题名，再优先下载数据库原始文件。原导出不可用时保存详情真实字段供
          AI
          和模板使用，不作为原始导入文件。遇到登录、验证或不明确结果会停在当前篇。
        </p>
        <Button
          size="sm"
          variant="outline"
          :disabled="
            locked ||
            queueWorking ||
            !!windowAction ||
            !savedSite ||
            changedSite ||
            flowDirty ||
            (!!sourceQueue &&
              ['running', 'paused', 'blocked', 'interrupted'].includes(
                sourceQueue.status,
              ))
          "
          @click="queueControl('start')"
          >获取当前负责人全部未处理论文的来源</Button
        >
        <div v-if="sourceQueue" class="space-y-1 text-xs">
          <p>
            {{ sourceQueue.owner }} · {{ sourceQueue.channel }} ·
            {{ sourceQueue.index }} / {{ sourceQueue.entries.length }} ·
            {{
              sourceQueue.status === "running"
                ? "获取中"
                : sourceQueue.status === "completed"
                  ? "已结束"
                  : sourceQueue.status === "cancelled"
                    ? "已取消"
                    : "已暂停，等待继续"
            }}
          </p>
          <div class="flex gap-2">
            <Button
              v-if="sourceQueue.status === 'running'"
              size="sm"
              variant="outline"
              :disabled="sourceQueue.pause_requested"
              @click="queueControl('pause')"
              >{{
                sourceQueue.pause_requested ? "本篇结束后暂停" : "暂停队列"
              }}</Button
            >
            <Button
              v-if="
                ['paused', 'blocked', 'interrupted'].includes(
                  sourceQueue.status,
                )
              "
              size="sm"
              variant="outline"
              :disabled="locked || queueWorking"
              @click="queueControl('resume')"
              >继续原队列</Button
            >
            <Button
              v-if="
                ['paused', 'blocked', 'interrupted'].includes(
                  sourceQueue.status,
                )
              "
              size="sm"
              variant="outline"
              :disabled="locked || queueWorking"
              @click="queueControl('cancel')"
              >取消剩余队列</Button
            >
          </div>
          <p v-if="lastQueueResult?.error" class="text-muted-foreground">
            {{ lastQueueResult?.error?.message }}
          </p>
        </div>
        <p v-if="queueError" class="text-xs text-destructive">
          {{ queueError }}
        </p>
      </div>
      <details class="rounded-md border p-2">
        <summary class="cursor-pointer text-[11px]">
          渠道页面变化时调整自动步骤
        </summary>
        <p class="my-2 text-[11px] text-muted-foreground">
          默认识别页面实际检索和导出文字。需要适配特定页面时，保存本渠道定位条件；search_url
          可使用 {title} 和 {doi}，export_steps
          填写实际控件与文字。配置不包含可执行脚本。
        </p>
        <Textarea
          v-model="flowText"
          :disabled="locked || !!windowAction"
          class="min-h-48 font-mono text-xs"
          aria-label="渠道自动步骤配置"
        />
        <p v-if="flowError" class="text-xs text-destructive">{{ flowError }}</p>
        <Button
          size="sm"
          variant="outline"
          :disabled="locked || !!windowAction || !savedSite || changedSite"
          @click="saveFlow"
          >保存渠道步骤</Button
        >
      </details>
      <div
        v-for="a in acquisitions"
        :key="a.id"
        class="rounded-md border p-2 text-[11px]"
      >
        <p>
          {{ a.state === "captured" ? "详情来源" : "原始导出" }} ·
          {{
            {
              export_pending: "请求已记录",
              downloading: "正在下载",
              completed: "已确认原下载",
              unknown: "结果待确认",
              failed: "本次未完成",
              captured: "已保存，待选择记录",
            }[a.state]
          }}
        </p>
        <p v-if="a.error" class="text-muted-foreground">
          {{ a.error.message }}
        </p>
        <select
          v-if="a.state === 'unknown'"
          v-model="selectedExportReceipt[a.id]"
          class="h-8 w-full rounded-md border bg-background p-1"
          aria-label="选择原会话下载文件"
        >
          <option :value="undefined">自动核对唯一原下载</option>
          <option
            v-for="d in downloads.filter(
              (d) => d.session.id === a.session.id && d.state === 'completed',
            )"
            :key="d.id"
            :value="d.id"
          >
            {{ d.original_name }}
          </option>
        </select>
        <Button
          v-if="!['failed', 'captured'].includes(a.state)"
          size="sm"
          variant="outline"
          :disabled="!!windowAction"
          @click="recheck(a.id)"
          >检查原下载结果</Button
        >
        <Button
          v-if="a.state === 'captured'"
          size="sm"
          variant="outline"
          :disabled="locked || !!windowAction"
          @click="previewCapture(a.id)"
          >重新打开保存的详情来源</Button
        >
        <details v-if="a.state === 'unknown'" class="mt-2">
          <summary class="cursor-pointer">原下载未完成，另行处理</summary>
          <p class="my-2 text-muted-foreground">
            先关闭原会话所有窗口并核对下载。如果原会话有已完成文件，应先采用它；取消仅保留历史记录，不删除文件、不修改论文状态。
          </p>
          <Input
            v-model="exportReviewNote"
            placeholder="填写原下载核对情况，至少 6 字"
            :disabled="locked"
          />
          <Button
            size="sm"
            variant="outline"
            :disabled="locked || exportReviewNote.trim().length < 6"
            @click="cancelExport(a.id)"
            >核对后取消原导出记录</Button
          >
        </details>
      </div>
      <div
        v-if="windows.length"
        class="space-y-2 border-t pt-2"
        aria-label="本篇来源窗口"
      >
        <p class="text-[11px] text-muted-foreground">
          本篇已打开的窗口（包含其他渠道）。网页打开不代表登录或访问权限已经确认。
        </p>
        <div
          v-for="window in windows"
          :key="window.label"
          class="space-y-1 rounded-md border p-2 text-[11px]"
        >
          <p>
            {{ window.channel_label }} ·
            {{ window.popup ? "登录或来源弹窗" : "来源窗口"
            }}<span v-if="window.downloading"> · 正在下载</span>
          </p>
          <p>{{ window.title }}</p>
          <p class="break-all text-muted-foreground">
            {{ window.url || "地址暂不可读取" }}
          </p>
          <p
            v-if="!window.current_record"
            class="text-amber-700 dark:text-amber-400"
          >
            窗口属于旧名单记录，请按当前名单重新打开后下载。
          </p>
          <p
            v-if="!window.current_site"
            class="text-amber-700 dark:text-amber-400"
          >
            窗口的入口或下载来源配置已改变，请使用已保存配置重新打开后下载。
          </p>
          <div class="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="outline"
              :disabled="
                locked ||
                !!windowAction ||
                window.downloading ||
                !window.current_record ||
                !window.current_site ||
                changedSite ||
                flowDirty ||
                window.channel !== channel
              "
              @click="acquire('capture', window)"
              >读取当前详情来源</Button
            >
            <Button
              size="sm"
              :disabled="
                locked ||
                !!windowAction ||
                window.downloading ||
                !window.current_record ||
                !window.current_site ||
                changedSite ||
                flowDirty ||
                window.channel !== channel
              "
              @click="acquire('export', window)"
              >下载当前论文原始文件</Button
            >
            <Button
              v-if="window.channel === 'cnki'"
              size="sm"
              :disabled="
                locked ||
                !!windowAction ||
                window.downloading ||
                !window.current_record ||
                !window.current_site
              "
              @click="cnki('capture', window)"
              >获取当前详情页题录</Button
            >
            <Button
              size="sm"
              variant="outline"
              :disabled="!!windowAction"
              @click="manage(window, false)"
              >切回网页</Button
            >
            <Button
              size="sm"
              variant="outline"
              :disabled="!!windowAction"
              @click="manage(window, true)"
              >{{
                window.downloading ? "关闭并保留未确认下载" : "关闭窗口"
              }}</Button
            >
          </div>
        </div>
      </div>
      <div
        v-for="d in downloads"
        :key="d.id"
        class="space-y-1 rounded-md bg-muted/40 p-2 text-[11px]"
      >
        <p>{{ d.original_name }} · {{ labels[d.state] || d.state }}</p>
        <p class="break-all text-muted-foreground">{{ d.path }}</p>
        <p v-if="d.error" class="text-destructive">{{ d.error.message }}</p>
        <p
          v-if="d.session.input_hash !== task.input_hash"
          class="text-amber-700"
        >
          这是旧名单版本的下载。
        </p>
        <details>
          <summary class="cursor-pointer">来源与文件版本</summary>
          <p class="break-all">{{ d.page_url }}</p>
          <p class="break-all">SHA256：{{ d.sha256 || "未确认" }}</p>
        </details>
        <Button
          v-if="/\.(xls|xlsx)$/i.test(d.path)"
          size="sm"
          variant="outline"
          :disabled="
            locked ||
            d.state !== 'completed' ||
            d.session.input_hash !== task.input_hash
          "
          @click="preview(d, true)"
          >另存为 XLSX 并选择论文记录</Button
        >
        <Button
          size="sm"
          variant="outline"
          :disabled="
            locked ||
            d.state !== 'completed' ||
            d.session.input_hash !== task.input_hash
          "
          @click="preview(d)"
          >选择文件中的论文记录</Button
        >
      </div>
      <p v-if="!downloads.length" class="text-[11px] text-muted-foreground">
        本渠道暂无下载记录。导出完成后刷新此列表，文件不会保存到系统 Downloads。
      </p>
    </template>
  </section>
</template>
