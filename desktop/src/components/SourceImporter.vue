<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { Task, SourcePage } from "@/types";
import type { DesktopCommand } from "@/services/desktop";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Upload, RefreshCw } from "@lucide/vue";
import SourceBrowserPanel from "@/components/SourceBrowserPanel.vue";
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
  delimiter = ref("comma");
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
const textMode = computed(() => data.value?.draft.format === "txt");
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
watch([() => props.task.id, () => props.task.revision], reset);
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
  data.value = value;
  sheet.value = value.sheet;
  header.value = value.header_row;
  selectedRow.value = null;
  titleColumn.value = "";
  doiColumn.value = "";
  wosColumn.value = "";
  confirmed.value = false;
}
async function preview() {
  const { id, revision } = props.task;
  const result = await props.run<SourcePage>(
    "preview_source_file",
    {
      id,
      channel: channel.value,
      options: { encoding: encoding.value, delimiter: delimiter.value },
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
</script>
<template>
  <div class="space-y-3 rounded-xl border p-4" data-testid="source-importer">
    <h3 class="text-xs font-medium">读取其他渠道的原始来源</h3>
    <p class="text-[11px] leading-6 text-muted-foreground">
      先取得数据库原始 Excel / CSV /
      TXT。选择准确记录后保存全部字段；本地来源读取不表示数据库自动导出或平台导入已完成。
    </p>
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
        >CSV 分隔符<select
          v-model="delimiter"
          aria-label="来源 CSV 分隔符"
          class="native-select mt-2 w-full"
          :disabled="locked"
        >
          <option value="comma">逗号</option>
          <option value="tab">制表符</option>
          <option value="semicolon">分号</option>
        </select></label
      >
    </div>
    <SourceBrowserPanel
      :task="task"
      :channel="channel"
      :locked="locked"
      :encoding="encoding"
      :delimiter="delimiter"
      :run="run"
      @preview="captured"
    />
    <Button
      class="w-full"
      variant="outline"
      size="sm"
      :disabled="locked || !channels.length"
      @click="preview"
      ><Upload />选择原始来源文件</Button
    >
    <Button
      class="w-full"
      variant="ghost"
      size="sm"
      :disabled="locked"
      @click="page(0, true)"
      ><RefreshCw />继续上次来源预览</Button
    >
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
      <label class="field"
        >原始工作表<select
          v-model="sheet"
          aria-label="来源工作表"
          class="native-select mt-2 w-full"
          :disabled="locked"
        >
          <option v-for="s in data.sheets" :key="s.name">{{ s.name }}</option>
        </select></label
      >
      <label v-if="!textMode" class="field"
        >原始表头行号<Input
          type="number"
          min="1"
          v-model.number="header"
          aria-label="来源表头行号"
          :disabled="locked"
          class="mt-2"
      /></label>
      <Button variant="outline" size="sm" :disabled="locked" @click="page(0)"
        >按所选工作表和表头读取</Button
      >
      <p class="text-[11px] text-muted-foreground">
        {{ data.total }} {{ textMode ? "行文本" : "条记录" }}，每页最多 50
        条。请明确选择本篇，程序不会默认第一条。
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
            :disabled="locked"
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
              :disabled="locked" /></label
          ><label class="field"
            >结束行<Input
              v-model.number="textEnd"
              type="number"
              min="1"
              aria-label="来源文本结束行"
              :disabled="locked"
          /></label>
        </div>
        <label class="field"
          >范围中的完整原文题名<Input
            v-model="textTitle"
            aria-label="来源文本原文题名"
            :disabled="locked"
        /></label>
      </template>
      <template v-else>
        <label class="field"
          >题名列<select
            v-model="titleColumn"
            aria-label="来源题名列"
            class="native-select mt-2 w-full"
            :disabled="locked"
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
            :disabled="locked"
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
            :disabled="locked"
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
