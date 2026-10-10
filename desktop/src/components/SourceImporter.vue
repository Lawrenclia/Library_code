<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type {
  Task,
  SourcePage,
  SourceCandidates,
  SourceReuseChoice,
  SourceReuseOptions,
} from "@/types";
import type { DesktopCommand } from "@/services/desktop";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Upload, RefreshCw } from "@lucide/vue";
import SourceBrowserPanel from "@/components/SourceBrowserPanel.vue";
import DoiSourcePanel from "@/components/DoiSourcePanel.vue";
const props = defineProps<{
  task: Task;
  locked: boolean;
  channels: { id: string; label: string; formats: string[] }[];
  run: <T>(
    command: DesktopCommand,
    args?: Record<string, unknown>,
    success?: string,
  ) => Promise<T | undefined>;
}>();
const channel = ref("cnki"),
  encoding = ref("utf-8"),
  delimiter = ref("comma"),
  textTable = ref(false);
const data = ref<SourcePage | null>(null),
  sheet = ref(""),
  header = ref(1),
  selectedRow = ref<number | null>(null);
const titleColumn = ref(""),
  doiColumn = ref(""),
  wosColumn = ref("");
const textStart = ref(1),
  textEnd = ref(1),
  textTitle = ref("");
const url = ref(""),
  note = ref(""),
  confirmed = ref(false);
const textMode = computed(() =>
  data.value?.layout
    ? data.value.layout === "text"
    : data.value?.draft.format === "txt",
);
const reusing = computed(() => !!data.value?.draft.origin);
const reusable = ref<SourceReuseOptions | null>(null);
const originals = computed(() =>
  props.task.evidence
    .filter((e) => e.kind === "external_metadata")
    .map((e) => {
      try {
        return JSON.parse(e.text) as {
          title: string;
          channel: string;
          original_name: string;
          archive_path: string;
          sha256: string;
          input_hash: string;
          record_fingerprint: string;
        };
      } catch {
        return null;
      }
    })
    .filter(Boolean),
);
const selected = computed(() =>
  data.value?.rows.find((r) => r.row === selectedRow.value),
);
const matches = ref<SourceCandidates | null>(null);
const matchesCurrent = computed(() => {
  const found = matches.value;
  return (
    !!found &&
    !!data.value &&
    found.task_id === props.task.id &&
    found.input_hash === props.task.input_hash &&
    found.task_revision === props.task.revision &&
    found.preview_id === data.value.draft.id &&
    found.sha256 === data.value.draft.sha256 &&
    found.sheet === sheet.value &&
    found.header_row === Number(header.value) &&
    String(found.title_column) === titleColumn.value &&
    (found.doi_column === null ? "" : String(found.doi_column)) ===
      doiColumn.value &&
    (found.wos_column === null ? "" : String(found.wos_column)) ===
      wosColumn.value
  );
});
const ready = computed(
  () =>
    !!data.value &&
    confirmed.value &&
    !!note.value.trim() &&
    sheet.value === data.value.sheet &&
    (textMode.value || header.value === data.value.header_row) &&
    (textMode.value
      ? !!textTitle.value.trim() && textEnd.value >= textStart.value
      : selectedRow.value !== null && titleColumn.value !== ""),
);
function reset() {
  matches.value = null;
  data.value = null;
  selectedRow.value = null;
  titleColumn.value = "";
  doiColumn.value = "";
  wosColumn.value = "";
  textStart.value = 1;
  textEnd.value = 1;
  textTitle.value = "";
  url.value = "";
  note.value = "";
  confirmed.value = false;
}
watch([() => props.task.id, () => props.task.revision], () => {
  reset();
  reusable.value = null;
});
watch(
  [
    selectedRow,
    titleColumn,
    doiColumn,
    wosColumn,
    textStart,
    textEnd,
    textTitle,
    url,
    note,
    sheet,
    header,
  ],
  () => {
    confirmed.value = false;
  },
);
function accept(value: SourcePage | undefined, id: string, revision: number) {
  if (
    !value ||
    value.cancelled ||
    props.task.id !== id ||
    props.task.revision !== revision
  )
    return;
  const fresh = data.value?.draft.id !== value.draft.id;
  if (fresh) reset();
  data.value = value;
  sheet.value = value.sheet;
  header.value = value.header_row;
  if (fresh && value.draft.channel === "cnki" && value.sheet === "CNKI 题录") {
    const column = (names: string[]) => {
      const found = value.columns.filter((c) => names.includes(c.name));
      return found.length === 1 ? String(found[0]!.column) : "";
    };
    titleColumn.value = column(["%T", "T1"]);
    doiColumn.value = column(["%R", "DO", "DI"]);
  }
  if (value.draft.origin) {
    const original = value.draft.origin;
    const s = original.selection;
    sheet.value = s.sheet;
    header.value = s.header_row;
    selectedRow.value = s.row;
    titleColumn.value = s.title_column === null ? "" : String(s.title_column);
    doiColumn.value = s.doi_column === null ? "" : String(s.doi_column);
    wosColumn.value = s.wos_column === null ? "" : String(s.wos_column);
    textStart.value = s.row;
    textEnd.value = s.end_row;
    textTitle.value = s.text_title;
    if (fresh) url.value = original.source_url;
  }
  confirmed.value = false;
}
async function loadReusable() {
  const { id, revision } = props.task;
  const result = await props.run<SourceReuseOptions>(
    "source_reuse_options",
    { id },
    "已读取相同标识符的原始来源。",
  );
  if (
    result &&
    result.task_id === id &&
    result.task_revision === revision &&
    props.task.id === id &&
    props.task.revision === revision
  )
    reusable.value = result;
}
async function reuse(choice: SourceReuseChoice) {
  const { id, revision } = props.task;
  if (
    reusable.value?.task_id !== id ||
    reusable.value.task_revision !== revision
  )
    return;
  const result = await props.run<SourcePage>(
    "preview_reused_source",
    {
      id,
      expectedRevision: revision,
      sourceId: choice.source_id,
      sourceInputHash: choice.source_input_hash,
      sourceFingerprint: choice.source_fingerprint,
      evidenceId: choice.original.evidence_id,
      evidenceHash: choice.original.evidence_hash,
    },
    "已读取所选原文件，请为本条填写对应依据并确认。",
  );
  accept(result, id, revision);
}
async function preview(resave = false) {
  const { id, revision } = props.task;
  const result = await props.run<SourcePage>(
    "preview_source_file",
    {
      id,
      channel: channel.value,
      resave,
      options: {
        encoding: encoding.value,
        delimiter: delimiter.value,
        text_table: textTable.value,
      },
    },
    "原文件已保存，选择其中一条论文记录并核对表头。",
  );
  accept(result, id, revision);
}
function captured(page: SourcePage, source: string) {
  accept(page, props.task.id, props.task.revision);
  if (data.value?.draft.id === page.draft.id) url.value = source;
}
async function page(number = 0, resume = false) {
  const { id, revision } = props.task;
  const result = await props.run<SourcePage>(
    "source_file_page",
    {
      id,
      previewId: resume ? null : data.value?.draft.id,
      sheet: resume ? null : sheet.value,
      headerRow: resume ? 0 : Number(header.value),
      page: number,
    },
    "已读取原始来源预览。",
  );
  accept(result, id, revision);
}
async function attach() {
  if (!ready.value || !data.value) return;
  const row = textMode.value ? Number(textStart.value) : selectedRow.value;
  await props.run(
    "attach_source_file",
    {
      id: props.task.id,
      previewId: data.value.draft.id,
      selection: {
        sheet: data.value.sheet,
        header_row: Number(header.value),
        row,
        end_row: textMode.value ? Number(textEnd.value) : row,
        title_column:
          titleColumn.value === "" ? null : Number(titleColumn.value),
        doi_column: doiColumn.value === "" ? null : Number(doiColumn.value),
        wos_column: wosColumn.value === "" ? null : Number(wosColumn.value),
        text_title: textTitle.value,
      },
      sourceUrl: url.value.trim(),
      note: note.value.trim(),
      confirmed: confirmed.value,
    },
    "原始来源已绑定，全部所选字段可用于 AI 分类和模板填写；归属与平台阶段仍须独立核验。",
  );
}
async function locate() {
  if (
    !data.value ||
    textMode.value ||
    reusing.value ||
    titleColumn.value === ""
  )
    return;
  const { id, revision } = props.task;
  const draft = data.value.draft;
  const criteria = JSON.stringify([
    sheet.value,
    header.value,
    titleColumn.value,
    doiColumn.value,
    wosColumn.value,
  ]);
  matches.value = null;
  selectedRow.value = null;
  confirmed.value = false;
  const result = await props.run<SourceCandidates>(
    "locate_source_records",
    {
      id,
      previewId: draft.id,
      mapping: {
        sheet: sheet.value,
        header_row: Number(header.value),
        row: Number(header.value) + 1,
        end_row: Number(header.value) + 1,
        title_column: Number(titleColumn.value),
        doi_column: doiColumn.value === "" ? null : Number(doiColumn.value),
        wos_column: wosColumn.value === "" ? null : Number(wosColumn.value),
        text_title: "",
      },
    },
    "已检查整张来源表。请选择并核验本篇候选；此结果不表示已绑定或已确认归属。",
  );
  if (
    result &&
    props.task.id === id &&
    props.task.revision === revision &&
    data.value?.draft.id === draft.id &&
    criteria ===
      JSON.stringify([
        sheet.value,
        header.value,
        titleColumn.value,
        doiColumn.value,
        wosColumn.value,
      ])
  )
    matches.value = result;
}
async function showCandidate(hit: SourceCandidates["candidates"][number]) {
  if (!matchesCurrent.value || !data.value) return;
  const found = matches.value;
  if (data.value.page !== hit.page) await page(hit.page);
  if (
    matches.value === found &&
    matchesCurrent.value &&
    data.value?.rows.some((r) => r.row === hit.row)
  ) {
    selectedRow.value = hit.basis === "conflict" ? null : hit.row;
    confirmed.value = false;
  }
}
const conflictNames: Record<string, string> = {
  doi_invalid: "DOI 格式无效",
  doi_mismatch: "DOI 与名单冲突",
  wos_invalid: "WOS ID 格式无效",
  wos_mismatch: "WOS ID 与名单冲突",
};
</script>
<template>
  <div class="space-y-3 rounded-xl border p-4" data-testid="source-importer">
    <h3 class="text-xs font-medium">读取其他渠道的原始来源</h3>
    <p class="text-[11px] leading-6 text-muted-foreground">
      先取得数据库原始 Excel / CSV /
      TXT。选择准确记录后保存全部字段；本地来源读取不表示数据库自动导出或平台导入已完成。
    </p>
    <DoiSourcePanel :task="task" :locked="locked" :run="run" />
    <label class="field"
      >来源渠道<select
        v-model="channel"
        aria-label="原始来源渠道"
        class="native-select mt-2 w-full"
        :disabled="locked"
      >
        <option v-for="c in channels" :key="c.id" :value="c.id">
          {{ c.label }}
        </option>
      </select></label
    >
    <p class="text-[10px] text-muted-foreground">
      渠道、编码和分隔符用于选择新文件；已保存的预览保留原读取设置。
    </p>
    <div class="grid grid-cols-2 gap-2">
      <label class="field"
        >文本编码<select
          v-model="encoding"
          aria-label="来源文本编码"
          class="native-select mt-2 w-full"
          :disabled="locked"
        >
          <option value="utf-8">UTF-8 / BOM 自动识别</option>
          <option value="gb18030">GB18030 / GBK</option>
          <option value="utf-16le">UTF-16 LE</option>
          <option value="utf-16be">UTF-16 BE</option>
        </select></label
      >
      <label class="field"
        >表格分隔符<select
          v-model="delimiter"
          aria-label="来源表格分隔符"
          class="native-select mt-2 w-full"
          :disabled="locked"
        >
          <option value="comma">逗号</option>
          <option value="tab">制表符</option>
          <option value="semicolon">分号</option>
        </select></label
      >
    </div>
    <label class="flex items-start gap-2 text-xs leading-6">
      <input
        v-model="textTable"
        type="checkbox"
        :disabled="locked"
        class="mt-1.5"
      />
      TXT 按分隔表格读取（适用于带表头的制表符、逗号或分号文件）
    </label>
    <p class="text-[10px] text-muted-foreground">
      普通文本保持按行读取；勾选后按上方分隔符读取全部列，仍需选择实际记录和身份列。
    </p>
    <SourceBrowserPanel
      :task="task"
      :channel="channel"
      :locked="locked"
      :encoding="encoding"
      :delimiter="delimiter"
      :text-table="textTable"
      :run="run"
      @preview="captured"
    />
    <Button
      class="w-full"
      variant="outline"
      size="sm"
      :disabled="locked || !channels.length"
      @click="preview(false)"
      ><Upload />选择原始来源文件</Button
    >
    <Button
      v-if="channel === 'cnki'"
      class="w-full"
      variant="outline"
      size="sm"
      :disabled="locked"
      @click="preview(true)"
      >选择 CNKI Excel 并另存为 XLSX</Button
    >
    <Button
      class="w-full"
      variant="ghost"
      size="sm"
      :disabled="locked"
      @click="page(0, true)"
      ><RefreshCw />继续上次来源预览</Button
    >
    <div class="space-y-2 rounded-lg border p-3" data-testid="source-reuse">
      <p class="text-xs font-medium">复用其他 SA 的同篇原文件</p>
      <p class="text-[11px] leading-5 text-muted-foreground">
        按 DOI 或 WOS ID 查找，明确选择后核对原记录。各条 SA
        的认领和处理进度分别保存。
      </p>
      <Button
        variant="outline"
        size="sm"
        :disabled="locked"
        @click="loadReusable"
        >查找可复用来源</Button
      >
      <template v-if="reusable">
        <p
          v-if="!reusable.choices.length"
          class="text-[11px] text-muted-foreground"
        >
          没有标识符一致且原文件完整的来源。可选择新的原始文件。
        </p>
        <div
          v-for="choice in reusable.choices"
          :key="`${choice.source_id}:${choice.original.evidence_id}`"
          class="space-y-1 rounded-lg border p-2"
        >
          <p class="break-all text-xs">{{ choice.original.receipt.title }}</p>
          <p class="text-[11px] text-muted-foreground">
            SA {{ choice.source_id }} ·
            {{
              channels.find((c) => c.id === choice.original.receipt.channel)
                ?.label || choice.original.receipt.channel
            }}
            · {{ choice.original.receipt.original_name }}
          </p>
          <p class="break-all text-[11px] text-muted-foreground">
            {{ choice.original.receipt.doi || choice.original.receipt.wos }} ·
            {{ choice.original.receipt.selection.sheet }} 第
            {{ choice.original.receipt.selection.row }} 行
          </p>
          <p
            v-if="choice.original.receipt.source_url"
            class="break-all text-[11px] text-muted-foreground"
          >
            {{ choice.original.receipt.source_url }}
          </p>
          <Button
            variant="outline"
            size="sm"
            :disabled="locked"
            @click="reuse(choice)"
            >预览这份来源</Button
          >
        </div>
        <p
          v-for="problem in reusable.problems"
          :key="problem.source_id"
          class="text-[11px] text-destructive"
        >
          SA {{ problem.source_id }}：{{ problem.error.message }}
        </p>
      </template>
    </div>
    <div
      v-if="data"
      class="space-y-3 border-t pt-3"
      data-testid="source-preview"
    >
      <p class="break-all text-xs">
        {{ data.draft.original_name }} · {{ data.actual_encoding }}
      </p>
      <p class="text-[11px] text-muted-foreground">
        本次预览渠道：{{
          channels.find((c) => c.id === data?.draft.channel)?.label ||
          data.draft.channel
        }}
      </p>
      <p class="break-all font-mono text-[10px] text-muted-foreground">
        SHA-256：{{ data.draft.sha256 }}
      </p>
      <div
        v-if="data.draft.resave"
        class="space-y-1 rounded border p-2 text-[11px] leading-5"
      >
        <p>
          已另存完整表格，原下载文件保留。请继续选择准确论文记录并核对关键字段。
        </p>
        <p class="break-all">原件：{{ data.draft.resave.original_path }}</p>
        <p class="break-all">另存：{{ data.draft.resave.saved_path }}</p>
        <p>另存不会确认论文身份、交大归属或平台导入成功。</p>
      </div>
      <div
        v-if="data.draft.options?.export_response"
        class="space-y-1 rounded border p-2 text-[11px] leading-5"
      >
        <p>
          这份文件来自内置会话的题录响应，用于分类和填表。已保留全部字段与原始响应；直接导入机构库时，请使用网页原始导出或对应模板。
        </p>
        <template v-if="data.acquisition">
          <p class="break-all">题录：{{ data.acquisition.saved_path }}</p>
          <p class="break-all">
            原始响应：{{ data.acquisition.response_path }}
          </p>
          <p
            v-if="data.acquisition.missing.length"
            class="text-amber-700 dark:text-amber-400"
          >
            导出字段缺项：{{
              data.acquisition.missing.join("、")
            }}。需从其他可靠来源补充，AI 不得猜写。
          </p>
        </template>
      </div>
      <p
        v-if="data.draft.origin"
        class="text-[11px] leading-5 text-muted-foreground"
      >
        来源 SA
        {{ data.draft.origin.task_id }}
        的原文件快照。已保留原绑定记录及全部字段，请填写本条对应依据；如需选择其他记录，请重新选择原文件。
      </p>
      <label class="field"
        >原始工作表<select
          v-model="sheet"
          aria-label="来源工作表"
          class="native-select mt-2 w-full"
          :disabled="locked || reusing"
        >
          <option v-for="s in data.sheets" :key="s.name">{{ s.name }}</option>
        </select></label
      >
      <label v-if="!textMode" class="field"
        >原始表头记录号<Input
          type="number"
          min="1"
          v-model.number="header"
          aria-label="来源表头记录号"
          :disabled="locked || reusing"
          class="mt-2"
      /></label>
      <Button variant="outline" size="sm" :disabled="locked" @click="page(0)"
        >按所选工作表和表头读取</Button
      >
      <p class="text-[11px] text-muted-foreground">
        {{ data.total }} {{ textMode ? "行文本" : "条记录" }}，每页最多 50
        条。请明确选择本篇，程序不会默认第一条。
      </p>
      <p
        v-if="data.draft.format === 'txt' && !textMode"
        class="text-[11px] text-muted-foreground"
      >
        {{
          data.draft.options?.tagged_format === "cnki"
            ? "CNKI 标记题录按论文分行；保留全部标签、重复作者和多行内容，以及原始完整记录。"
            : "本预览按分隔表格读取。记录号对应表格行；引号内换行保留在同一字段，不按物理文本行拆分。"
        }}
      </p>
      <div class="max-h-56 space-y-1 overflow-auto rounded border p-2">
        <label
          v-for="row in data.rows"
          :key="row.row"
          class="flex cursor-pointer items-start gap-2 rounded p-2 text-[11px] leading-6 hover:bg-muted/40"
        >
          <input
            v-if="!textMode"
            type="radio"
            :value="row.row"
            v-model="selectedRow"
            :disabled="locked || reusing"
            aria-label="选择来源记录"
          />
          <span class="shrink-0 text-muted-foreground">{{ row.row }}</span
          ><span class="min-w-0 break-all">{{
            row.values.slice(0, 3).join(" · ")
          }}</span>
        </label>
      </div>
      <div class="flex items-center justify-between">
        <Button
          variant="ghost"
          size="sm"
          :disabled="locked || data.page === 0"
          @click="page(data.page - 1)"
          >上一页</Button
        ><span class="text-[10px]"
          >第 {{ data.page + 1 }} /
          {{ Math.max(1, Math.ceil(data.total / 50)) }} 页</span
        ><Button
          variant="ghost"
          size="sm"
          :disabled="locked || (data.page + 1) * 50 >= data.total"
          @click="page(data.page + 1)"
          >下一页</Button
        >
      </div>
      <template v-if="textMode">
        <div class="grid grid-cols-2 gap-2">
          <label class="field"
            >开始行<Input
              v-model.number="textStart"
              type="number"
              min="1"
              aria-label="来源文本开始行"
              :disabled="locked || reusing" /></label
          ><label class="field"
            >结束行<Input
              v-model.number="textEnd"
              type="number"
              min="1"
              aria-label="来源文本结束行"
              :disabled="locked || reusing"
          /></label>
        </div>
        <label class="field"
          >范围中的完整原文题名<Input
            v-model="textTitle"
            aria-label="来源文本原文题名"
            :disabled="locked || reusing"
        /></label>
      </template>
      <template v-else>
        <label class="field"
          >题名列<select
            v-model="titleColumn"
            aria-label="来源题名列"
            class="native-select mt-2 w-full"
            :disabled="locked || reusing"
          >
            <option value="">请选择真实题名列</option>
            <option
              v-for="c in data.columns"
              :key="c.column"
              :value="String(c.column)"
            >
              {{ c.label }}
            </option>
          </select></label
        >
        <label class="field"
          >DOI 列<select
            v-model="doiColumn"
            aria-label="来源 DOI 列"
            class="native-select mt-2 w-full"
            :disabled="locked || reusing"
          >
            <option value="">原文件未提供 / 待核验</option>
            <option
              v-for="c in data.columns"
              :key="c.column"
              :value="String(c.column)"
            >
              {{ c.label }}
            </option>
          </select></label
        >
        <label class="field"
          >WOS ID 列<select
            v-model="wosColumn"
            aria-label="来源 WOS 列"
            class="native-select mt-2 w-full"
            :disabled="locked || reusing"
          >
            <option value="">原文件未提供 / 待核验</option>
            <option
              v-for="c in data.columns"
              :key="c.column"
              :value="String(c.column)"
            >
              {{ c.label }}
            </option>
          </select></label
        >
        <div class="space-y-2 rounded border p-3">
          <Button
            variant="outline"
            size="sm"
            :disabled="
              locked ||
              reusing ||
              titleColumn === '' ||
              sheet !== data.sheet ||
              header !== data.header_row
            "
            @click="locate"
            >按名单定位整张表中的候选</Button
          >
          <p class="text-[11px] leading-6 text-muted-foreground">
            先选择真实身份列。仅按完整题名或实际标识符匹配，不作模糊匹配，也不默认绑定第一条。
          </p>
          <template v-if="matchesCurrent && matches">
            <p class="text-xs">
              检查 {{ matches.examined }} 条记录，命中
              {{ matches.candidates.length }} 个候选；{{
                matches.unreadable.length
              }}
              条无法读取。
            </p>
            <p
              v-if="!matches.candidates.length"
              class="text-[11px] leading-6 text-muted-foreground"
            >
              所选列中未定位到候选。请核对列与题名；这不代表数据库没有该文献，也不作为零结果记录。
            </p>
            <div class="max-h-64 space-y-2 overflow-auto">
              <div
                v-for="hit in matches.candidates"
                :key="hit.row"
                class="rounded border p-2 text-[11px] leading-6"
              >
                <p class="break-all">第 {{ hit.row }} 条：{{ hit.title }}</p>
                <p class="break-all text-muted-foreground">
                  {{
                    [hit.doi, hit.wos].filter(Boolean).join(" · ") ||
                    "原记录未提供所选标识符"
                  }}
                </p>
                <p
                  :class="
                    hit.basis === 'conflict'
                      ? 'text-destructive'
                      : 'text-muted-foreground'
                  "
                >
                  {{
                    hit.basis === "conflict"
                      ? hit.conflicts
                          .map((key) => conflictNames[key] || key)
                          .join("；")
                      : hit.basis === "identifier"
                        ? "标识符命中，仍须核对完整记录"
                        : "仅题名命中，身份仍须核验"
                  }}
                </p>
                <Button
                  variant="ghost"
                  size="sm"
                  :disabled="locked"
                  @click="showCandidate(hit)"
                  >{{
                    hit.basis === "conflict"
                      ? "查看冲突记录"
                      : "查看并选择此候选"
                  }}</Button
                >
              </div>
            </div>
            <details v-if="matches.unreadable.length">
              <summary class="text-xs">查看未读取的行</summary>
              <p
                v-for="problem in matches.unreadable"
                :key="problem.row"
                class="my-1 text-[11px] leading-6 text-destructive"
              >
                第 {{ problem.row }} 条：{{ problem.error.message }}
              </p>
            </details>
          </template>
        </div>
        <details v-if="selected">
          <summary>查看所选记录的全部字段</summary>
          <div
            v-for="(value, i) in selected.values"
            :key="i"
            class="my-2 rounded bg-muted/30 p-2 text-[11px] leading-6"
          >
            <p class="text-muted-foreground">{{ data.columns[i]?.label }}</p>
            <p class="break-all whitespace-pre-wrap">
              {{ value || "原字段为空" }}
            </p>
          </div>
        </details>
      </template>
      <label class="field"
        >数据库记录链接（有则填写）<Input
          v-model="url"
          aria-label="原始来源记录链接"
          :disabled="locked"
          placeholder="未提供时保留实际归档路径"
          class="mt-2"
      /></label>
      <label class="field"
        >与本条论文对应的具体依据<Textarea
          v-model="note"
          aria-label="来源绑定依据"
          :disabled="locked"
          class="mt-2"
      /></label>
      <label class="flex items-start gap-2 text-[11px] leading-6"
        ><input
          v-model="confirmed"
          type="checkbox"
          :disabled="locked"
          class="mt-1"
        />已核对所选记录或文本范围与本条论文一致</label
      >
      <Button
        class="w-full"
        size="sm"
        :disabled="locked || !ready"
        @click="attach"
        >绑定本条原始来源</Button
      >
    </div>
    <div
      v-for="(r, i) in originals"
      :key="i"
      class="space-y-2 rounded bg-muted/30 p-3 text-[11px] leading-6"
    >
      <p class="font-medium">{{ r?.title }}</p>
      <p>
        {{ r?.original_name }} ·
        {{ channels.find((c) => c.id === r?.channel)?.label || r?.channel }}
      </p>
      <p class="break-all font-mono text-[10px]">
        {{ r?.archive_path }}<br />SHA-256：{{ r?.sha256 }}
      </p>
      <p class="text-muted-foreground">
        原始来源已绑定；文献身份、归属与平台处理分别核验。历史来源保留在记录中。
      </p>
    </div>
  </div>
</template>
