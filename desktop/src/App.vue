<script setup lang="ts">
import { computed, ref } from "vue";
import {
  FileSpreadsheet,
  Download,
  Upload,
  Search,
  RefreshCw,
  Database,
  ArrowUpRight,
  ShieldCheck,
  Files,
  CheckCircle2,
  AlertCircle,
  LoaderCircle,
  Pause,
  BookOpen,
  Sparkles,
  Link2,
  Users,
  FolderOpen,
  Plus,
  ChevronRight,
  FileText,
  Check,
  Settings2,
  Maximize2,
  Minimize2,
} from "@lucide/vue";
import WorkbenchShell from "@/components/WorkbenchShell.vue";
import WorkflowOverview from "@/components/WorkflowOverview.vue";
import SourceImporter from "@/components/SourceImporter.vue";
import AiQueuePanel from "@/components/AiQueuePanel.vue";
import LegacyMaterials from "@/components/LegacyMaterials.vue";
import TaskTable from "@/components/TaskTable.vue";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  CardDescription,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Progress } from "@/components/ui/progress";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { useWorkbench } from "@/composables/useWorkbench";
const {
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
  editableAuthors,
  prepareMetadata,
  saveMetadata,
  libraryTitle,
  librarySearch,
  libraryCheckReady,
  searchLibrary,
  saveApi,
  register,
  classify,
  fill,
  confirmation,
  settleConfirmation,
} = useWorkbench();
const pageTitles: Record<string, string> = {
  workflow: "每一步，都有清晰的去向。",
  tasks: "把每一篇成果，整理清楚。",
  materials: "模板与提交材料",
  settings: "模型与工作空间",
};
const detailExpanded = ref(false);
const filters = [
  { id: "all", label: "全部任务" },
  { id: "zero", label: "零匹配" },
  { id: "review", label: "待核验" },
  { id: "skipped", label: "跳过项" },
  { id: "done", label: "已处理" },
];
const metrics = computed(() => [
  {
    label: "名单任务",
    value: counts.value.all,
    caption: "当前负责人范围",
    icon: Files,
  },
  {
    label: "零匹配",
    value: counts.value.zero,
    caption: "先查来源，再确认原因",
    icon: Search,
  },
  {
    label: "待核验",
    value: counts.value.review,
    caption: "需要依据或结果回读",
    icon: ShieldCheck,
  },
  {
    label: "SA 已处理",
    value: counts.value.done,
    caption: "已保存最终处理状态",
    icon: CheckCircle2,
  },
]);
const progress = computed(() =>
  counts.value.all
    ? Math.round((counts.value.done / counts.value.all) * 100)
    : 0,
);
const aiFields = computed(() => {
  const fields = current.value?.classification?.fields;
  return fields && typeof fields === "object" ? Object.entries(fields) : [];
});
function clearFilters() {
  search.value = "";
  filter.value = "all";
  owner.value = "";
}
function frameworkAction(action: string) {
  if (action === "import")
    return run("import_roster", {}, "名单已读取，原表与历史记录保留。");
  if (action === "report")
    return run("export_report", {}, "任务与来源报告已导出。");
  if (action === "folder") return run("open_folder");
  if (action === "review") {
    owner.value = "";
    search.value = "";
    filter.value = "review";
    page.value = "tasks";
    return;
  }
  page.value = action;
}
function frameworkBrowser(role: string, entry?: string) {
  return run(
    "open_browser",
    { role, entry: entry || null },
    entry
      ? "数据库入口已打开。请通过图书馆访问 WOS，完成机构登录后回到工作台。"
      : "工作页已打开，请在窗口内确认机构访问。",
  );
}
</script>

<template>
  <WorkbenchShell
    v-model:page="page"
    :count="workspace.tasks.length"
    :running="workspace.running"
    :configured="api.configured"
    @folder="run('open_folder')"
  >
    <section class="mb-5 flex flex-wrap items-center justify-between gap-4">
      <div>
        <div
          class="mb-1.5 flex items-center gap-1.5 text-[10px] font-medium uppercase tracking-widest text-primary"
        >
          <span class="size-1 rounded-full bg-primary"></span
          >{{
            page === "workflow"
              ? "Workflow overview"
              : page === "tasks"
                ? "Research workspace"
                : page === "materials"
                  ? "Submission materials"
                  : "Workspace settings"
          }}
        </div>
        <h1 class="text-xl font-semibold tracking-tight lg:text-2xl">
          {{ pageTitles[page] }}
        </h1>
        <p class="mt-2 text-xs text-muted-foreground">
          {{
            page === "workflow"
              ? "名单、来源、核验和平台执行在同一框架中衔接。"
              : page === "tasks"
                ? "按照机构知识库比对流程，核验来源、整理材料、完成处理。"
                : page === "materials"
                  ? "优先保留数据库原始导出，缺失内容按实际模板准备。"
                  : "配置模型 API，管理本地工作文件与访问状态。"
          }}
        </p>
      </div>
      <div v-if="page === 'tasks'" class="flex gap-2">
        <Button
          variant="outline"
          size="sm"
          :disabled="locked"
          @click="run('export_report', {}, '任务与来源报告已导出。')"
          ><Download />导出来源报告</Button
        ><Button
          size="sm"
          :disabled="locked"
          @click="run('import_roster', {}, '名单已读取，历史任务与来源保留。')"
          ><Plus />导入名单</Button
        >
      </div>
    </section>
    <div
      class="message-bar mb-5"
      :class="error ? 'message-error' : pending ? 'message-pending' : ''"
      role="status"
    >
      <AlertCircle v-if="error" class="size-4 shrink-0" /><LoaderCircle
        v-else-if="pending"
        class="size-4 shrink-0 animate-spin"
      /><CheckCircle2 v-else class="size-4 shrink-0" /><span
        class="min-w-0 flex-1 text-xs leading-5"
        >{{ message }}</span
      ><Button
        variant="ghost"
        size="icon-xs"
        aria-label="刷新工作台"
        @click="refresh"
        ><RefreshCw class="size-3.5"
      /></Button>
    </div>
    <WorkflowOverview
      v-if="page === 'workflow'"
      :workspace="workspace"
      :configured="api.configured"
      :templates="schemas.length"
      :locked="locked"
      @action="frameworkAction"
      @browser="frameworkBrowser"
    />
    <template v-else-if="page === 'tasks'">
      <div class="mb-5 grid grid-cols-4 gap-3">
        <Card
          v-for="metric in metrics"
          :key="metric.label"
          class="gap-0 rounded-xl py-0 shadow-none"
          ><CardContent class="px-4 py-4"
            ><div class="flex items-center justify-between">
              <span class="text-xs text-muted-foreground">{{
                metric.label
              }}</span
              ><component
                :is="metric.icon"
                class="size-3.5 text-muted-foreground/65"
              />
            </div>
            <div class="mb-1 mt-3 text-2xl font-semibold tabular-nums">
              {{ metric.value }}
            </div>
            <p class="text-[10px] text-muted-foreground">
              {{ metric.caption }}
            </p></CardContent
          ></Card
        >
      </div>
      <div
        class="mb-5 flex flex-wrap items-center gap-3 rounded-xl border bg-card px-4 py-3"
      >
        <div class="mr-auto">
          <div class="flex items-center gap-1.5 text-xs font-medium">
            <Database class="size-3.5" />数据库与工作页
          </div>
          <p class="mt-1 text-[10px] text-muted-foreground">
            首次使用，请在窗口内完成机构登录。
          </p>
        </div>
        <Button
          v-for="[role, label] in [
            ['wos', 'WOS 数据库'],
            ['sa', 'SA 比对'],
            ['library', '查本库'],
            ['import', '导入管理'],
          ]"
          :key="role"
          variant="outline"
          size="sm"
          :disabled="pending"
          @click="
            run(
              'open_browser',
              { role },
              '工作页已打开，请完成登录并回到工作台。',
            )
          "
          ><span
            class="size-1.5 rounded-full"
            :class="
              workspace.browsers[role]?.open
                ? 'bg-emerald-500'
                : 'bg-muted-foreground/45'
            "
          ></span
          >{{ label }}<ArrowUpRight class="size-3! text-muted-foreground"
        /></Button>
      </div>
      <div class="mb-4 flex flex-wrap items-center gap-2">
        <label class="flex items-center gap-2 text-xs text-muted-foreground"
          >负责人<select
            aria-label="负责人"
            v-model="owner"
            :disabled="locked"
            class="native-select w-32"
          >
            <option value="">全部负责人</option>
            <option v-for="o in owners" :key="o">{{ o }}</option>
          </select></label
        >
        <div class="relative min-w-40 flex-1">
          <Search
            class="absolute top-2.5 left-3 size-3.5 text-muted-foreground"
          /><Input
            v-model="search"
            class="h-9 pl-9 text-xs"
            placeholder="搜索论文、DOI 或 SA ID"
            aria-label="搜索任务"
            @keydown.esc="search = ''"
          />
        </div>
        <Button
          size="sm"
          :disabled="locked || !owner || queuePending"
          @click="queue(false)"
          ><Download />检索 / 下载 WOS</Button
        ><Button
          v-if="workspace.running || pending"
          size="sm"
          variant="outline"
          @click="
            run('pause_queue', {}, '已请求暂停，当前步骤结束后保存进度。')
          "
          ><Pause />暂停后续任务</Button
        ><Button
          v-else
          size="sm"
          variant="outline"
          :disabled="locked || !owner || queuePending"
          @click="queue(true)"
          >检索跳过项</Button
        >
      </div>
      <AiQueuePanel
        :workspace="workspace"
        :owner="owner"
        :locked="locked"
        :configured="api.configured"
        :templates="schemas"
        :run="run"
      />
      <section
        v-if="workspace.download_queue"
        aria-label="下载队列进度"
        class="mb-4 rounded-xl border bg-card p-3 text-xs"
      >
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div class="flex flex-wrap items-center gap-2">
            <Badge variant="secondary">{{
              queueStatusNames[workspace.download_queue.status]
            }}</Badge>
            <span
              >{{ workspace.download_queue.owner }} ·
              {{
                workspace.download_queue.retry_skipped ? "跳过论文" : "待补论文"
              }}</span
            >
            <span class="text-muted-foreground"
              >已记录 {{ workspace.download_queue.cursor }} /
              {{ workspace.download_queue.targets.length }} 篇 · 剩余
              {{
                workspace.download_queue.targets.length -
                workspace.download_queue.cursor
              }}
              篇</span
            >
          </div>
          <div
            v-if="queuePending && workspace.download_queue.status !== 'running'"
            class="flex gap-2"
          >
            <Button size="sm" :disabled="locked" @click="resumeQueue"
              >继续原队列</Button
            >
            <Button
              size="sm"
              variant="outline"
              :disabled="locked"
              @click="cancelQueue"
              >结束原范围</Button
            >
          </div>
        </div>
        <Progress
          class="mt-3 h-1.5"
          :model-value="
            workspace.download_queue.targets.length
              ? (workspace.download_queue.cursor /
                  workspace.download_queue.targets.length) *
                100
              : 0
          "
        />
        <p
          v-if="
            workspace.download_queue.pause_requested &&
            workspace.download_queue.status === 'running'
          "
          class="mt-2 text-muted-foreground"
        >
          已请求暂停，当前步骤结束后保存进度。
        </p>
        <p
          v-if="workspace.download_queue.last_error"
          class="mt-2 text-amber-700 dark:text-amber-400"
        >
          {{ workspace.download_queue.last_error.message }}
        </p>
        <details v-if="workspace.download_queue.outcomes.length" class="mt-2">
          <summary class="cursor-pointer text-muted-foreground">
            查看本轮逐篇结果
          </summary>
          <ul class="mt-2 max-h-44 space-y-1 overflow-auto">
            <li
              v-for="result in workspace.download_queue.outcomes"
              :key="result.id"
            >
              SA {{ result.id }} ·
              {{
                result.status === "downloaded"
                  ? "文件已保存"
                  : result.status === "not_executed"
                    ? "任务变化，未执行"
                    : "待核验"
              }}<span v-if="result.error"> · {{ result.error.message }}</span>
            </li>
          </ul>
        </details>
      </section>
      <div class="mb-4 flex items-center gap-4 border-b">
        <button
          v-for="item in filters"
          :key="item.id"
          class="filter-tab"
          :class="{ 'filter-tab-active': filter === item.id }"
          @click="filter = item.id"
        >
          {{ item.label
          }}<span
            v-if="item.id === 'review' && counts.review"
            class="ml-1.5 rounded bg-amber-100 px-1.5 text-[10px] text-amber-700 dark:bg-amber-950"
            >{{ counts.review }}</span
          >
        </button>
        <div
          class="ml-auto hidden w-24 items-center gap-2 pb-3 text-[10px] text-muted-foreground lg:flex"
        >
          <Progress :model-value="progress" class="h-1.5" /><span
            >{{ progress }}%</span
          >
        </div>
      </div>
      <div
        class="workbench-grid"
        :class="{ 'workbench-detail-expanded': detailExpanded && current }"
      >
        <TaskTable
          v-show="!detailExpanded || !current"
          :tasks="tasks"
          :selected="selectedId"
          :stage-names="stageNames"
          :has-roster="!!workspace.tasks.length"
          @select="selectedId = $event"
          @import="run('import_roster')"
          @clear="clearFilters"
        />
        <section
          id="task-details"
          class="detail-panel overflow-hidden rounded-xl border bg-card"
          aria-label="论文详情"
        >
          <template v-if="current"
            ><div class="border-b px-5 pt-5 pb-4">
              <div class="mb-3 flex items-center justify-between gap-2">
                <Badge variant="outline" class="rounded-md text-[10px]">{{
                  routeNames[current.route]
                }}</Badge>
                <div class="flex items-center gap-2">
                  <span class="text-[10px] text-muted-foreground"
                    >第 {{ current.record.row }} 行</span
                  >
                  <Button
                    variant="ghost"
                    size="sm"
                    class="h-7 text-[11px]"
                    :aria-label="
                      detailExpanded ? '返回任务列表' : '展开论文详情'
                    "
                    :aria-expanded="detailExpanded"
                    aria-controls="task-details"
                    @click="detailExpanded = !detailExpanded"
                  >
                    <Minimize2 v-if="detailExpanded" class="size-3.5!" />
                    <Maximize2 v-else class="size-3.5!" />
                    {{ detailExpanded ? "返回列表" : "展开" }}
                  </Button>
                </div>
              </div>
              <h2 class="line-clamp-3 text-sm font-semibold leading-6">
                {{ current.record.title }}
              </h2>
              <p class="mt-2 text-[10px] text-muted-foreground">
                {{ current.record.owner
                }}<span class="mx-2 text-muted-foreground/40">·</span
                ><span class="font-mono">{{ current.id }}</span>
              </p>
            </div>
            <Tabs
              :model-value="detailTab"
              @update:model-value="detailTab = String($event)"
              class="border-b px-4 py-2"
              ><TabsList class="grid h-8 w-full grid-cols-4 bg-muted/60"
                ><TabsTrigger value="review" class="text-[11px]"
                  >核验</TabsTrigger
                ><TabsTrigger value="sources" class="text-[11px]"
                  >来源</TabsTrigger
                ><TabsTrigger value="actions" class="text-[11px]"
                  >平台操作</TabsTrigger
                ><TabsTrigger value="ai" class="text-[11px]"
                  >AI 建议</TabsTrigger
                ></TabsList
              ></Tabs
            >
            <div class="detail-scroll space-y-4 p-5">
              <div
                v-if="current.last_error"
                class="rounded-lg border border-amber-200/80 bg-amber-50 p-3 text-xs leading-6 text-amber-800 dark:border-amber-900 dark:bg-amber-950/30 dark:text-amber-300"
              >
                <div class="mb-1 flex items-center gap-1.5 font-medium">
                  <AlertCircle class="size-3.5" />这篇需要核对
                </div>
                {{ current.last_error.message }}
                <details>
                  <summary class="mt-1 cursor-pointer text-[10px] opacity-65">
                    查看原因代码
                  </summary>
                  <code>{{ current.last_error.code }}</code>
                </details>
              </div>
              <template v-if="detailTab === 'review'">
                <div
                  v-if="current.pending_input"
                  class="space-y-4 rounded-xl border border-amber-200 bg-amber-50/30 p-4 dark:border-amber-900"
                >
                  <h3 class="text-sm font-medium">新名单版本待核验</h3>
                  <p class="text-xs leading-6 text-muted-foreground">
                    旧记录与执行历史保留。先回读实时
                    SA，核对新字段与责任人安排，再采用新版本。旧业务结论不会自动沿用。
                  </p>
                  <div class="overflow-auto rounded-lg border bg-card">
                    <table class="w-full text-left text-xs">
                      <thead>
                        <tr class="border-b">
                          <th class="p-2">字段</th>
                          <th class="p-2">旧版</th>
                          <th class="p-2">新名单</th>
                        </tr>
                      </thead>
                      <tbody>
                        <tr
                          v-for="field in versionDiffs"
                          :key="field.label"
                          class="border-b last:border-0"
                        >
                          <td class="p-2">{{ field.label }}</td>
                          <td class="p-2 break-all">
                            {{ field.before || "空" }}
                          </td>
                          <td class="p-2 break-all">
                            {{ field.after || "空" }}
                          </td>
                        </tr>
                      </tbody>
                    </table>
                  </div>
                  <p class="path text-[10px] text-muted-foreground">
                    {{ current.pending_input.source_file || "导入的名单" }} ·
                    {{ current.pending_input.input_hash }}
                  </p>
                  <Button
                    variant="outline"
                    :disabled="locked || current.stage === 'unknown'"
                    @click="prepareVersion"
                    ><RefreshCw />读取新版本与实时 SA</Button
                  >
                  <template v-if="versionPrepared">
                    <details>
                      <summary>查看本次实时字段</summary>
                      <pre class="max-h-60 overflow-auto text-[10px]">{{
                        JSON.stringify(versionPrepared.snapshot, null, 2)
                      }}</pre>
                    </details>
                    <label class="field"
                      >版本与责任人安排来源<Input
                        v-model="versionSource"
                        class="mt-2 text-xs"
                    /></label>
                    <label class="field"
                      >新版本的具体核验依据<Textarea
                        v-model="versionProof"
                        class="mt-2 text-xs"
                    /></label>
                    <label class="flex gap-2 text-xs leading-6"
                      ><input
                        type="checkbox"
                        v-model="versionConfirmed"
                      />已核对新名单事实、责任人安排及本次回读</label
                    >
                    <Button
                      :disabled="
                        locked ||
                        !versionConfirmed ||
                        !versionSource.trim() ||
                        !versionProof.trim()
                      "
                      @click="acceptVersion"
                      ><Check />确认采用新名单版本</Button
                    >
                  </template>
                </div>
                <Button
                  v-if="
                    current.stage === 'unknown' &&
                    current.pending_action === 'save_metadata'
                  "
                  class="w-full"
                  variant="outline"
                  size="sm"
                  :disabled="locked"
                  @click="step('verify_metadata')"
                  >核验上次本库字段保存结果</Button
                >
                <p class="helper">
                  {{
                    current.record.matches === 0
                      ? "先确认零匹配的原因。只有交大成果、本库确实缺失，才进入导入准备。"
                      : "核对通讯作者、第一作者、第一单位等实际待处理原因，再保存处理结论。"
                  }}
                </p>
                <div class="rounded-lg bg-muted/50 p-3">
                  <p class="mb-1 text-[10px] text-muted-foreground">
                    名单中的待处理原因
                  </p>
                  <p class="text-xs leading-6">
                    {{ current.record.reason || "名单未提供" }}
                  </p>
                </div>
                <section
                  v-if="
                    current.record.matches >= 1 ||
                    ['missing', 'corrected_existing'].includes(current.route)
                  "
                  class="space-y-3 rounded-xl border p-3"
                >
                  <p class="text-xs font-semibold">逐项核对待处理原因</p>
                  <p class="helper">
                    需要实时 SA
                    仅有一个匹配。重复任务先合并，补充任务先推送并关联。本库字段修正后先刷新，再保存本项结论。
                  </p>
                  <Button
                    class="w-full"
                    size="sm"
                    variant="outline"
                    :disabled="
                      locked || ['unknown', 'completed'].includes(current.stage)
                    "
                    @click="prepareIssues"
                    ><RefreshCw />读取 / 刷新逐项核对清单</Button
                  >
                  <template v-if="issueChecklist">
                    <p
                      v-if="!issueChecklist.requirements.length"
                      class="helper"
                    >
                      实时字段没有待处理原因；最终完成前仍会再次回读。
                    </p>
                    <label v-else class="field"
                      >本次核对的原因<select
                        aria-label="本次核对的原因"
                        v-model="issueKey"
                        class="native-select mt-2 w-full"
                      >
                        <option value="">请选择一项，不自动选择</option>
                        <option
                          v-for="item in issueChecklist.requirements"
                          :key="item.key"
                          :value="item.key"
                        >
                          {{ item.label }}
                        </option>
                      </select></label
                    >
                    <template v-if="selectedIssue">
                      <div class="space-y-2 rounded-lg bg-muted/50 p-3 text-xs">
                        <p class="text-[10px] text-muted-foreground">
                          首次读取的 SA 值
                        </p>
                        <pre
                          class="max-h-36 overflow-auto whitespace-pre-wrap break-all"
                          >{{
                            selectedIssue.sa ??
                            "角色或字段未唯一定位，需核对工号与原文"
                          }}</pre>
                        <p class="text-[10px] text-muted-foreground">
                          本库实时值
                        </p>
                        <pre
                          class="max-h-36 overflow-auto whitespace-pre-wrap break-all"
                          >{{
                            selectedIssue.library ??
                            "字段未定位，不能按相似姓名确认"
                          }}</pre>
                      </div>
                      <label class="field"
                        >本项处理结论<select
                          aria-label="本项处理结论"
                          v-model="issueOutcome"
                          class="native-select mt-2 w-full"
                        >
                          <option value="">请选择处理结论</option>
                          <option
                            v-for="option in issueOutcomes"
                            :key="option.value"
                            :value="option.value"
                          >
                            {{ option.label }}
                          </option>
                        </select></label
                      >
                      <Button
                        v-if="issueOutcome === 'sa_correct'"
                        class="w-full"
                        size="sm"
                        variant="outline"
                        :disabled="locked"
                        @click="step('open_metadata')"
                        >打开对应条目的编辑页<ArrowUpRight
                      /></Button>
                      <p v-if="issueOutcome === 'sa_correct'" class="helper">
                        依据原文修改对应字段并在平台保存，然后刷新清单。程序回读确认本库值等于核对后的
                        SA 值，才保存解决结论。
                      </p>
                      <section
                        v-if="
                          issueOutcome === 'sa_correct' &&
                          [
                            'corresponding_author',
                            'first_author',
                            'first_institution',
                          ].includes(issueKey)
                        "
                        class="space-y-3 rounded-lg border p-3"
                      >
                        <p class="text-xs font-medium">
                          按已核对的来源修改作者与单位信息
                        </p>
                        <p class="helper">
                          先打开并登录学者管理；程序按完整工号查询实际身份，再读取编辑页。明确选择作者、填写下方本项来源、依据和备注后，才能确认保存。
                        </p>
                        <Button
                          variant="outline"
                          size="sm"
                          class="w-full"
                          :disabled="locked"
                          @click="
                            run(
                              'open_browser',
                              { role: 'scholar' },
                              '学者管理已打开，请完成机构登录。',
                            )
                          "
                          >打开学者管理<ArrowUpRight
                        /></Button>
                        <Button
                          variant="outline"
                          size="sm"
                          class="w-full"
                          :disabled="
                            locked ||
                            ['unknown', 'completed'].includes(current.stage)
                          "
                          @click="prepareMetadata"
                          >按工号读取可编辑作者字段</Button
                        >
                        <template v-if="metadataPrepared">
                          <label class="field"
                            >本项修改方式<select
                              aria-label="本项修改方式"
                              v-model="metadataOperation"
                              class="native-select mt-2 w-full"
                              @change="chooseMetadataOperation"
                            >
                              <option
                                v-if="issueKey !== 'first_institution'"
                                value="role"
                              >
                                {{
                                  issueKey === "first_author"
                                    ? "共同第一作者标记"
                                    : "通讯作者标记"
                                }}
                              </option>
                              <option
                                v-if="
                                  issueKey === 'first_author' &&
                                  metadataPrepared.result.can_reorder_authors
                                "
                                value="author_order"
                              >
                                调整完整作者署名顺序
                              </option>
                              <option
                                v-if="issueKey === 'first_institution'"
                                value="institution_order"
                                :disabled="
                                  !metadataPrepared.result
                                    .can_reorder_institutions
                                "
                              >
                                调整完整单位顺序与作者单位编号
                              </option>
                            </select></label
                          >
                          <p class="helper">
                            {{
                              metadataPrepared.result.scholar.nameCn ||
                              metadataPrepared.result.scholar.nameEn
                            }}
                            · {{ metadataPrepared.result.staff_id }} ·
                            {{ metadataPrepared.result.item_id }}
                          </p>
                          <label class="field"
                            >本库待修改的作者行<select
                              aria-label="本库待修改的作者行"
                              v-model="metadataAuthor"
                              class="native-select mt-2 w-full"
                            >
                              <option :value="null">
                                请选择核对后的作者，不自动选择
                              </option>
                              <option
                                v-for="author in editableAuthors"
                                :key="author.id"
                                :value="author.index"
                              >
                                {{ author.fullname }} · 署名顺序
                                {{ author.order }}
                              </option>
                            </select></label
                          >
                          <p v-if="!editableAuthors.length" class="helper">
                            当前表单没有本项所需的完整可编辑字段，请在实际编辑页核对。
                          </p>
                          <div
                            v-if="
                              metadataOperation !== 'role' &&
                              editableAuthors.length
                            "
                            class="space-y-2"
                          >
                            <p class="helper">
                              按照原文核对下方完整顺序。原有身份与角色标记保留；单位调整同时更新每位作者的单位编号。
                            </p>
                            <ol class="space-y-2" aria-label="修改后的完整顺序">
                              <li
                                v-for="(row, position) in orderedMetadataRows"
                                :key="row.index"
                                class="flex items-center gap-2 rounded-md border p-2 text-xs"
                              >
                                <span class="min-w-0 flex-1 break-words"
                                  >{{ position + 1 }}. {{ row.label }}</span
                                >
                                <Button
                                  variant="outline"
                                  size="sm"
                                  :aria-label="`上移 ${row.label}`"
                                  :disabled="locked || position === 0"
                                  @click="moveMetadataRow(position, -1)"
                                  >上移</Button
                                >
                                <Button
                                  variant="outline"
                                  size="sm"
                                  :aria-label="`下移 ${row.label}`"
                                  :disabled="
                                    locked ||
                                    position === metadataOrder.length - 1
                                  "
                                  @click="moveMetadataRow(position, 1)"
                                  >下移</Button
                                >
                              </li>
                            </ol>
                          </div>
                          <Button
                            class="w-full"
                            size="sm"
                            :disabled="
                              locked ||
                              metadataAuthor === null ||
                              !metadataOrderChanged ||
                              !issueSource.trim() ||
                              !issueProof.trim() ||
                              !issueNote.trim() ||
                              ['unknown', 'completed'].includes(current.stage)
                            "
                            @click="saveMetadata"
                            >确认保存本项修改并回读</Button
                          >
                        </template>
                      </section>
                      <p v-if="issueOutcome === 'claimed'" class="helper">
                        先在“平台操作”按完整工号认领并回读；仅姓名相似或认领按钮点击成功不能解决本项。
                      </p>
                      <p v-if="issueOutcome === 'same_paper'" class="helper">
                        打开 DOI
                        等原始来源核对两边是否同一篇。不同论文保留待核验；不修改为“已处理”。
                      </p>
                      <label class="field"
                        >本项原文或来源<Input
                          v-model="issueSource"
                          class="mt-2 text-xs"
                          placeholder="原文链接、DOI 对应页面或可靠材料"
                      /></label>
                      <label class="field"
                        >本项具体依据<Textarea
                          v-model="issueProof"
                          class="mt-2 min-h-16 text-xs"
                          placeholder="来源中的作者角色、单位顺序或论文身份"
                      /></label>
                      <label class="field"
                        >本项核对备注<Textarea
                          v-model="issueNote"
                          class="mt-2 min-h-16 text-xs"
                          placeholder="经核对是 / 不是通讯作者等具体结论"
                      /></label>
                      <Button
                        class="w-full"
                        size="sm"
                        :disabled="
                          locked ||
                          !issueOutcome ||
                          !issueSource.trim() ||
                          !issueProof.trim() ||
                          !issueNote.trim() ||
                          ['unknown', 'completed'].includes(current.stage)
                        "
                        @click="saveIssue"
                        >保存本项并核验回读</Button
                      >
                    </template>
                  </template>
                  <div
                    v-if="current.issue_reviews?.length"
                    class="space-y-2 rounded-lg bg-muted/50 p-3"
                  >
                    <p class="text-xs font-medium">已保存的逐项结论</p>
                    <p
                      v-for="item in current.issue_reviews"
                      :key="item.key"
                      class="text-xs leading-5"
                    >
                      {{
                        current.issue_plan?.requirements.find(
                          (i) => i.key === item.key,
                        )?.label || item.key
                      }}：{{ item.note }}
                    </p>
                    <p class="helper">
                      {{
                        issueReady
                          ? "每项均有记录；完成前重新核对实时字段。"
                          : "仍有原因需要分别核对。"
                      }}
                    </p>
                  </div>
                </section>
                <label class="field"
                  >核验分支<select
                    aria-label="核验分支"
                    v-model="review.route"
                    class="native-select mt-2 w-full"
                  >
                    <template v-if="current.record.matches === 0"
                      ><option value="missing">交大成果 · 本库确实缺失</option>
                      <option value="corrected_existing">
                        纠正题名后本库已有
                      </option>
                      <option value="non_sjtu">非交大成果</option>
                      <option value="not_found">未查询到文献</option></template
                    >
                    <option
                      v-else-if="current.record.matches === 1"
                      value="existing"
                    >
                      核对现有条目
                    </option>
                    <option v-else value="duplicate">重复条目核验</option>
                  </select></label
                >
                <section
                  v-if="
                    current.record.matches === 0 &&
                    ['missing', 'corrected_existing'].includes(review.route)
                  "
                  class="space-y-3 rounded-xl border bg-muted/20 p-4"
                >
                  <div class="flex items-center gap-2 text-xs font-semibold">
                    <Search class="size-4 text-primary" />先查机构库前端
                  </div>
                  <p class="helper">
                    使用来源中的正确题名检索；已知 DOI、WOS
                    号一并核查。结果逐条保留，不自动选择候选。
                  </p>
                  <label class="field"
                    >正确题名<Input
                      aria-label="查本库的正确题名"
                      v-model="libraryTitle"
                      class="mt-2 text-xs"
                  /></label>
                  <Button
                    class="w-full"
                    variant="outline"
                    size="sm"
                    :disabled="
                      locked ||
                      !libraryTitle.trim() ||
                      current.stage === 'completed'
                    "
                    @click="searchLibrary"
                    ><Search />查询本库并保存结果</Button
                  >
                  <template v-if="librarySearch">
                    <p class="helper">
                      上次实际查询：{{ librarySearch.target.title }} ·
                      {{ new Date(librarySearch.checked_at).toLocaleString() }}
                    </p>
                    <p
                      v-for="query in librarySearch.queries"
                      :key="query.kind"
                      class="text-xs leading-6 break-words"
                    >
                      {{ query.kind.toUpperCase() }}：{{ query.value }} ·
                      {{ query.total }} 条 / {{ query.pages.length }} 页
                    </p>
                    <p
                      v-if="!librarySearch.items.length"
                      class="text-xs leading-6 text-emerald-700 dark:text-emerald-400"
                    >
                      全部查询明确返回零条。请结合文献身份与交大归属证据确认分支。
                    </p>
                    <template v-else>
                      <label class="field"
                        >选择已核对的本库条目<select
                          aria-label="已核对的本库条目"
                          v-model="review.platform_id"
                          class="native-select mt-2 w-full"
                        >
                          <option value="">请选择；不默认第一条</option>
                          <option
                            v-for="item in librarySearch.items"
                            :key="item.id"
                            :value="item.id"
                          >
                            {{ item.id }} ·
                            {{ item.metadata.title.join(" / ") }}
                          </option>
                        </select></label
                      >
                      <div
                        v-for="item in librarySearch.items"
                        :key="item.id"
                        class="rounded-lg border bg-background p-3"
                      >
                        <p class="text-xs leading-6 break-words">
                          {{ item.metadata.title.join(" / ") }}
                        </p>
                        <p
                          class="font-mono text-[10px] text-muted-foreground break-all"
                        >
                          {{ item.id }} · {{ item.modelName || "条目" }}
                        </p>
                        <details class="mt-2">
                          <summary class="cursor-pointer text-xs text-primary">
                            查看原始结果字段
                          </summary>
                          <pre class="source-text mt-2">{{
                            JSON.stringify(item.metadata, null, 2)
                          }}</pre>
                        </details>
                      </div>
                      <p class="helper">
                        存在候选时先核对。零匹配且本库已有，请选择“纠正题名后本库已有”；推送后的任务请核实实际条目
                        ID。
                      </p>
                    </template>
                  </template>
                  <p v-else class="helper">
                    尚无真实查询记录。打不开前端时，先在“查本库”窗口完成机构访问。
                  </p>
                </section>
                <label
                  v-if="
                    current.record.matches !== 0 ||
                    !['missing', 'corrected_existing'].includes(review.route)
                  "
                  class="field"
                  >平台唯一号<Input
                    v-model="review.platform_id"
                    class="mt-2 text-xs"
                    placeholder="本库已有、合并或推送后的条目 ID"
                /></label>
                <div class="space-y-3 rounded-lg border p-3">
                  <label class="check-row"
                    ><input
                      type="checkbox"
                      v-model="review.identity_confirmed"
                    />已核实为目标文献</label
                  ><label class="check-row"
                    ><input
                      type="checkbox"
                      v-model="review.affiliation_confirmed"
                    />已核实交大署名与归属</label
                  ><label class="check-row"
                    ><input
                      type="checkbox"
                      v-model="review.library_checked"
                      :disabled="
                        current.record.matches === 0 &&
                        ['missing', 'corrected_existing'].includes(review.route)
                      "
                    />已使用正确题名 / 标识符检索机构库</label
                  ><label v-if="!requiresIssueChecklist" class="check-row"
                    ><input
                      type="checkbox"
                      v-model="review.issues_resolved"
                    />待处理原因已逐项解决</label
                  >
                  <p v-else class="helper">
                    {{
                      issueReady
                        ? "每个待处理原因均已有独立来源与回读记录。"
                        : "待处理原因尚未逐项核验，不能设置已处理。"
                    }}
                  </p>
                </div>
                <label class="field"
                  >来源网址或材料名称<Input
                    v-model="source"
                    class="mt-2 text-xs"
                    placeholder="原文、数据库记录或机构库检索页面" /></label
                ><label class="field"
                  >具体核验依据<Textarea
                    v-model="proof"
                    class="mt-2 min-h-20 text-xs leading-6"
                    placeholder="实际查到了什么，为什么作出这一判断" /></label
                ><label class="field"
                  >核验结论 / 平台备注<Textarea
                    v-model="review.note"
                    class="mt-2 min-h-16 text-xs leading-6"
                    placeholder="非交大、未查询到该文献、经核对是 / 不是通讯作者等" /></label
                ><Button
                  class="w-full"
                  size="sm"
                  :disabled="
                    locked ||
                    ['unknown', 'completed'].includes(current.stage) ||
                    (current.record.matches === 0 && !libraryCheckReady)
                  "
                  @click="saveReview"
                  ><ShieldCheck />保存核验结论</Button
                ></template
              >
              <template v-if="detailTab === 'sources'"
                ><div
                  v-if="current.artifact"
                  class="rounded-xl border bg-muted/30 p-4"
                >
                  <div class="mb-3 flex items-center gap-2">
                    <FileText class="size-4 text-primary" /><span
                      class="text-xs font-medium"
                      >数据库原始记录</span
                    >
                  </div>
                  <p class="text-xs font-medium leading-6">
                    {{ current.artifact.candidate.title }}
                  </p>
                  <p class="mt-1 text-[11px] text-muted-foreground">
                    {{ current.artifact.candidate.journal }}
                  </p>
                  <p class="mt-3 font-mono text-[10px] leading-6">
                    {{ current.artifact.candidate.wos }}<br />{{
                      current.artifact.candidate.doi
                    }}
                  </p>
                  <div class="my-3 flex flex-wrap gap-2">
                    <Badge variant="secondary" class="text-[10px]">{{
                      current.artifact.candidate.sjtu
                        ? "已识别交大署名"
                        : "署名待核验"
                    }}</Badge
                    ><Badge variant="outline" class="text-[10px]">{{
                      current.artifact.identity_confirmed
                        ? "身份已核实"
                        : "身份待核验"
                    }}</Badge>
                  </div>
                  <p class="path">{{ current.artifact.path }}</p>
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  class="w-full"
                  :disabled="locked"
                  @click="
                    run(
                      'adopt_file',
                      { id: current.id },
                      '原始文件已解析并保存。',
                    )
                  "
                  ><Upload />读取本地 WOS TXT</Button
                >
                <SourceImporter
                  :task="current"
                  :locked="locked"
                  :channels="workspace.framework?.channels || []"
                  :run="run"
                />
                <div
                  v-for="e in current.evidence"
                  :key="e.id"
                  class="evidence-item"
                >
                  <span
                    class="absolute top-1 left-0 size-2 rounded-full border-2 border-primary/60 bg-card"
                  ></span>
                  <p class="mb-2 text-[10px] text-muted-foreground">
                    {{ new Date(e.created).toLocaleString() }}
                  </p>
                  <p class="path mb-2">{{ e.source }}</p>
                  <details
                    v-if="
                      e.kind === 'external_metadata' ||
                      e.kind === 'legacy_material' ||
                      e.kind === 'legacy_history' ||
                      e.kind === 'ai_classification'
                    "
                    class="text-xs leading-6"
                  >
                    <summary>
                      {{
                        e.kind === "external_metadata"
                          ? "查看完整来源字段与绑定记录"
                          : e.kind === "ai_classification"
                            ? "AI 分类记录 · 查看完整建议与来源"
                            : "旧版历史资料 · 待复核，展开查看完整记录"
                      }}
                    </summary>
                    <p class="break-all whitespace-pre-wrap">{{ e.text }}</p>
                  </details>
                  <p v-else class="whitespace-pre-wrap text-xs leading-6">
                    {{ e.text }}
                  </p>
                </div>
                <p v-if="!current.evidence.length" class="helper">
                  暂无来源。先检索数据库、读取原始文件或填写核验依据。
                </p></template
              >
              <template v-if="detailTab === 'actions'"
                ><p class="helper">
                  上传、导入、推送和 SA 已处理分别核验。结果不明时，先回读平台。
                </p>
                <Button
                  variant="outline"
                  size="sm"
                  class="w-full"
                  :disabled="locked"
                  @click="step('read_sa')"
                  ><RefreshCw />回读当前 SA 记录</Button
                ><Button
                  v-if="
                    current.stage === 'unknown' &&
                    ['complete', 'link', 'submit_claim'].includes(
                      current.pending_action || '',
                    )
                  "
                  size="sm"
                  class="w-full"
                  :disabled="locked"
                  @click="step('verify_sa')"
                  >核验上次 SA 操作结果</Button
                >
                <p
                  v-if="
                    current.stage === 'unknown' &&
                    current.pending_action === 'link'
                  "
                  class="helper"
                >
                  核验关联前，请打开并登录 SA
                  后台与机构库前端。程序会重新查询原条目，确认来源和完整元数据未变化；关联成功后仍需逐项核对待处理原因。
                </p>
                <Button
                  v-if="
                    current.stage === 'unknown' &&
                    current.pending_action === 'legacy_sa'
                  "
                  size="sm"
                  class="w-full"
                  :disabled="locked"
                  @click="step('verify_legacy_sa')"
                  >只读核验旧版 SA 完成记录</Button
                >
                <div class="action-block">
                  <h3><Users class="size-3.5" />现有条目核对与认领</h3>
                  <Button
                    variant="outline"
                    size="sm"
                    :disabled="locked"
                    @click="step('open_metadata')"
                    >打开元数据编辑页<ArrowUpRight /></Button
                  ><Button
                    variant="outline"
                    size="sm"
                    :disabled="locked"
                    @click="step('prepare_claim')"
                    >读取可认领作者</Button
                  >
                  <div v-if="claim" class="rounded-lg bg-muted/50 p-3">
                    <p class="mb-3 text-xs">
                      学者：{{ claim.prepared.person.name }}<br /><span
                        class="text-muted-foreground"
                        >工号 {{ claim.prepared.staff_id }}</span
                      >
                    </p>
                    <label class="field"
                      >确认论文中的作者行<select
                        aria-label="确认论文中的作者行"
                        v-model="authorIndex"
                        class="native-select mt-2 w-full"
                      >
                        <option :value="null">请选择并核对署名</option>
                        <option
                          v-for="a in claim.prepared.authors.filter(
                            (a) => a.eligible && !a.scholarId,
                          )"
                          :key="a.index"
                          :value="a.index"
                        >
                          {{ a.order }}. {{ a.fullname }}
                        </option>
                      </select></label
                    >
                  </div>
                  <Button
                    v-if="claim"
                    size="sm"
                    :disabled="locked || authorIndex === null"
                    @click="step('submit_claim', true)"
                    >确认认领对象并提交</Button
                  >
                  <p class="helper">
                    先核对工号和文献署名，需要时新增有来源依据的别名，再读取作者并认领。
                  </p>
                </div>
                <div class="action-block">
                  <h3><Users class="size-3.5" />学者别名（按需）</h3>
                  <Button
                    variant="outline"
                    size="sm"
                    :disabled="locked"
                    @click="
                      run(
                        'open_browser',
                        { role: 'scholar' },
                        '学者管理页已打开，请完成机构登录。',
                      )
                    "
                    >打开学者管理<ArrowUpRight
                  /></Button>
                  <Button
                    variant="outline"
                    size="sm"
                    :disabled="locked || current.stage === 'unknown'"
                    @click="prepareAlias"
                    >按工号读取学者与别名</Button
                  >
                  <template v-if="aliases">
                    <div class="rounded-lg bg-muted/50 p-3 text-xs leading-6">
                      <p>
                        {{ aliases.scholar.nameCn || aliases.scholar.nameEn }} ·
                        工号 {{ aliases.staff_id }}
                      </p>
                      <p class="text-muted-foreground">
                        已有别名：{{
                          aliases.aliases.map((a) => a.nameAlias).join("、") ||
                          "暂无"
                        }}
                      </p>
                    </div>
                    <label class="field"
                      >来源中的真实署名<Input
                        v-model="aliasName"
                        class="mt-2 text-xs"
                        placeholder="逐字核对文献中的署名"
                    /></label>
                    <label class="field"
                      >署名来源依据<select
                        v-model="aliasEvidence"
                        aria-label="署名来源依据"
                        class="native-select mt-2 w-full"
                      >
                        <option value="">请选择包含此署名的来源</option>
                        <option
                          v-for="e in current.evidence.filter((e) =>
                            ['metadata', 'human_review'].includes(e.kind),
                          )"
                          :key="e.id"
                          :value="e.id"
                        >
                          {{ e.source }} · {{ e.kind }}
                        </option>
                      </select></label
                    >
                    <Button
                      size="sm"
                      :disabled="
                        locked ||
                        !aliasName.trim() ||
                        !aliasEvidence ||
                        ['unknown', 'completed'].includes(current.stage)
                      "
                      @click="step('add_alias', true)"
                      >保存并回读别名</Button
                    >
                  </template>
                  <Button
                    v-if="
                      current.stage === 'unknown' &&
                      current.pending_action === 'add_alias'
                    "
                    variant="outline"
                    size="sm"
                    :disabled="locked"
                    @click="step('verify_alias')"
                    >核验上次别名保存结果</Button
                  >
                  <p class="helper">
                    仅新增来源中真实存在的署名；默认姓名不变。保存结果未知时先回读。
                  </p>
                </div>
                <div v-if="current.route === 'duplicate'" class="action-block">
                  <h3><Files class="size-3.5" />重复条目核对与合并</h3>
                  <Button
                    variant="outline"
                    size="sm"
                    :disabled="locked"
                    @click="
                      run(
                        'open_browser',
                        { role: 'duplicate' },
                        '重复管理页已打开，请完成机构登录。',
                      )
                    "
                    >打开重复数据管理<ArrowUpRight
                  /></Button>
                  <Button
                    variant="outline"
                    size="sm"
                    :disabled="
                      locked || ['unknown', 'completed'].includes(current.stage)
                    "
                    @click="scanDuplicates"
                    >按题名读取重复候选</Button
                  >
                  <template v-if="duplicateScan">
                    <p class="helper">
                      读取 {{ duplicateScan.groups.length }} 组；页面题名相似度
                      {{
                        duplicateScan.title_similarity
                      }}。相似度只是检索条件，合并须核实为同一论文。
                    </p>
                    <label class="field"
                      >候选组<select
                        v-model="duplicateGroup"
                        aria-label="重复候选组"
                        class="native-select mt-2 w-full"
                      >
                        <option value="">请选择需要逐项核对的候选组</option>
                        <option
                          v-for="g in duplicateScan.groups"
                          :key="g.id"
                          :value="g.id"
                        >
                          {{ g.items[0]?.metadata.title[0] }} ·
                          {{ g.items.length }} 个条目 · {{ g.id }}
                        </option>
                      </select></label
                    >
                    <Button
                      variant="outline"
                      size="sm"
                      :disabled="
                        locked || !duplicateGroup || current.stage === 'unknown'
                      "
                      @click="prepareDuplicate"
                      >读取选定组的完整字段</Button
                    >
                  </template>
                  <template v-if="duplicatePrepared">
                    <div
                      v-for="item in duplicatePrepared.group.items"
                      :key="item.id"
                      class="rounded-lg border p-3 text-xs leading-6"
                    >
                      <p class="font-medium">
                        {{ item.metadata.title.join(" / ") }}
                      </p>
                      <p class="text-muted-foreground">
                        {{ item.model_name }} · ID {{ item.id }}
                      </p>
                      <details>
                        <summary class="text-primary">查看原始字段</summary>
                        <pre>{{ JSON.stringify(item.metadata, null, 2) }}</pre>
                      </details>
                    </div>
                    <label class="field"
                      >合并后主条目<select
                        v-model="mergeTarget"
                        aria-label="合并后主条目"
                        class="native-select mt-2 w-full"
                      >
                        <option value="">明确选择保留的条目</option>
                        <option
                          v-for="i in mergeCandidates"
                          :key="i.id"
                          :value="i.id"
                        >
                          {{ i.id }} · {{ i.metadata.title[0] }}
                        </option>
                      </select></label
                    >
                    <label class="field"
                      >本次被合并条目<select
                        v-model="mergeSource"
                        aria-label="本次被合并条目"
                        class="native-select mt-2 w-full"
                      >
                        <option value="">逐次选择一个条目</option>
                        <option
                          v-for="i in mergeCandidates"
                          :key="i.id"
                          :value="i.id"
                        >
                          {{ i.id }} · {{ i.metadata.title[0] }}
                        </option>
                      </select></label
                    >
                    <label class="field"
                      >同一论文的来源依据<select
                        v-model="mergeEvidence"
                        aria-label="合并来源依据"
                        class="native-select mt-2 w-full"
                      >
                        <option value="">先在核验页保存真实依据</option>
                        <option
                          v-for="e in current.evidence.filter((e) =>
                            ['metadata', 'human_review'].includes(e.kind),
                          )"
                          :key="e.id"
                          :value="e.id"
                        >
                          {{ e.source }}
                        </option>
                      </select></label
                    >
                    <label class="field"
                      >保留字段与差异核对<Textarea
                        v-model="mergeRetained"
                        class="mt-2 text-xs"
                        placeholder="说明核对了哪些字段、主条目选择依据，以及合并后需再次检查的差异。"
                    /></label>
                    <label class="check-row"
                      ><input
                        v-model="mergeIdentity"
                        type="checkbox"
                      />已依据来源核实为同一论文，并核对主条目与保留字段</label
                    >
                    <Button
                      size="sm"
                      :disabled="
                        locked ||
                        !mergeSource ||
                        !mergeTarget ||
                        mergeSource === mergeTarget ||
                        !mergeEvidence ||
                        !mergeRetained.trim() ||
                        !mergeIdentity ||
                        ['unknown', 'completed'].includes(current.stage)
                      "
                      @click="step('merge_duplicate', true)"
                      >确认本条合并并回读</Button
                    >
                  </template>
                  <Button
                    v-if="
                      current.stage === 'unknown' &&
                      current.pending_action === 'merge_duplicate'
                    "
                    variant="outline"
                    size="sm"
                    :disabled="locked"
                    @click="step('verify_duplicate')"
                    >核验上次合并结果</Button
                  >
                  <div
                    v-if="current.merges?.length"
                    class="rounded-lg bg-muted/50 p-3 text-xs leading-6"
                  >
                    <p class="font-medium">已核验的合并</p>
                    <p v-for="m in current.merges" :key="m.evidence_id">
                      {{ m.source_id }} → {{ m.target_id }}
                    </p>
                    <p class="helper">
                      来源页保留合并前后的完整字段和回读结果。
                    </p>
                  </div>
                  <p class="helper">
                    每次只合并两个明确条目。合并后回到 SA
                    核对剩余原因与唯一主条目，再认领或设置已处理。结果不明时先回读。
                  </p>
                </div>
                <div class="action-block">
                  <h3><Database class="size-3.5" />缺失成果补充</h3>
                  <div
                    class="mb-2 flex items-center gap-1 text-[10px] text-muted-foreground"
                  >
                    <span>文件核验</span><ChevronRight class="size-3" /><span
                      >上传</span
                    ><ChevronRight class="size-3" /><span>导入</span
                    ><ChevronRight class="size-3" /><span>推送</span>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    :disabled="
                      locked ||
                      current.route !== 'missing' ||
                      !libraryCheckReady ||
                      !['ready', 'downloaded'].includes(current.stage)
                    "
                    @click="step('import_upload', true)"
                    ><Upload />1. 上传核验文件</Button
                  ><Button
                    variant="outline"
                    size="sm"
                    :disabled="locked || current.stage !== 'uploaded'"
                    @click="step('import_submit', true)"
                    >2. 提交导入</Button
                  ><Button
                    variant="outline"
                    size="sm"
                    :disabled="locked || !current.artifact"
                    @click="step('verify_import')"
                    ><RefreshCw />回读上传 / 导入 / 推送结果</Button
                  ><Button
                    variant="outline"
                    size="sm"
                    :disabled="locked || current.stage !== 'imported'"
                    @click="step('import_push', true)"
                    >3. 按 PPT 设置推送</Button
                  >
                  <details>
                    <summary>查看 PPT 推送规则</summary>
                    <p class="mt-2 text-xs leading-6">{{ workspace.policy }}</p>
                  </details>
                </div>
                <div class="action-block">
                  <h3><CheckCircle2 class="size-3.5" />回到 SA 完成处理</h3>
                  <Button
                    variant="outline"
                    size="sm"
                    :disabled="
                      locked ||
                      !current.platform_id ||
                      current.stage === 'unknown'
                    "
                    @click="step('link', true)"
                    ><Link2 />关联已核验的平台唯一号</Button
                  ><Button
                    size="sm"
                    :disabled="
                      locked ||
                      !current.review ||
                      current.route === 'not_found' ||
                      ['unknown', 'completed'].includes(current.stage)
                    "
                    @click="step('complete', true)"
                    >核验后设置 SA 已处理</Button
                  >
                </div>
                <details v-if="current.sa_snapshot">
                  <summary>查看最近 SA 回读</summary>
                  <pre>{{ JSON.stringify(current.sa_snapshot, null, 2) }}</pre>
                </details></template
              >
              <template v-if="detailTab === 'ai'"
                ><div
                  class="rounded-xl border border-primary/15 bg-primary/5 p-4"
                >
                  <Sparkles class="mb-2 size-5 text-primary" />
                  <h3 class="text-xs font-medium">基于来源的分类与补全</h3>
                  <p class="mt-2 text-[11px] leading-6 text-muted-foreground">
                    AI
                    可先依据名单分类，再结合原始来源补全字段。仅名单依据的建议置信度为低，推荐渠道仍需确认收录和文件。实际归属和平台操作由核验流程确认。
                  </p>
                </div>
                <label class="field"
                  >填写模板（可选）<select
                    aria-label="填写模板"
                    v-model="templateId"
                    class="native-select mt-2 w-full"
                  >
                    <option value="">仅分类与渠道判断</option>
                    <option v-for="s in schemas" :key="s.id" :value="s.id">
                      {{ s.name }}
                    </option>
                  </select></label
                ><Button
                  class="w-full"
                  size="sm"
                  :disabled="locked || !api.configured"
                  @click="classify"
                  ><Sparkles />调用 API 分类 / 填写</Button
                >
                <p v-if="!api.configured" class="helper">
                  先在“模型与设置”中保存 API 密钥。
                </p>
                <template v-if="current.classification"
                  ><div class="rounded-lg border p-3">
                    <p class="text-xs font-medium">
                      {{ current.classification.type || "成果类型待判定" }}
                    </p>
                    <p class="mt-2 text-[11px] leading-6 text-muted-foreground">
                      {{ current.classification.reason }}
                    </p>
                    <p class="mt-2 text-[10px]">
                      建议渠道：{{
                        workspace.framework?.channels.find(
                          (c) => c.id === current?.classification?.channel,
                        )?.label ||
                        current.classification.channel ||
                        "待判定"
                      }}
                      · 待核实收录和文件
                    </p>
                    <p class="mt-2 text-[11px] leading-6">
                      模型置信度：{{
                        current.classification.confidence ||
                        "旧结果未记录，请重新分类"
                      }}
                      · 建议待复核
                    </p>
                    <p class="mt-2 text-[11px] leading-6 text-muted-foreground">
                      {{ current.classification.channel_reason }}
                    </p>
                    <p
                      v-if="
                        Array.isArray(current.classification.missing) &&
                        current.classification.missing.length
                      "
                      class="mt-2 text-xs leading-6"
                    >
                      仍需补充：{{ current.classification.missing.join("、") }}
                    </p>
                  </div>
                  <div
                    v-for="[name, value] in aiFields"
                    :key="name"
                    class="rounded-lg bg-muted/40 p-3"
                  >
                    <p class="mb-2 text-[10px] text-muted-foreground">
                      {{ name }}
                    </p>
                    <p class="whitespace-pre-wrap text-xs leading-6">
                      {{
                        typeof value === "object" && value
                          ? "value" in value
                            ? value.value
                            : value
                          : value
                      }}
                    </p>
                  </div>
                  <details>
                    <summary>来源引用与缺项</summary>
                    <pre>{{
                      JSON.stringify(current.classification, null, 2)
                    }}</pre>
                  </details>
                  <Button
                    v-if="templateId"
                    variant="outline"
                    size="sm"
                    class="w-full"
                    :disabled="locked"
                    @click="fill"
                    ><FileSpreadsheet />检查必填项并导出模板材料</Button
                  >
                  <div
                    v-if="materialValidation"
                    class="space-y-2 rounded-lg border p-3"
                    data-testid="material-validation"
                  >
                    <p class="text-xs font-medium">
                      {{
                        materialValidation.ready
                          ? "模板校验通过"
                          : "材料草稿待补充或核对"
                      }}
                    </p>
                    <p class="break-all text-[11px] text-muted-foreground">
                      {{ materialValidation.path }}
                    </p>
                    <p v-if="materialValidation.missing.length" class="text-xs">
                      必填缺项：{{ materialValidation.missing.join("、") }}
                    </p>
                    <div
                      v-for="p in materialValidation.invalid || []"
                      :key="p.column + p.rule_source"
                      class="rounded bg-destructive/5 p-2 text-[11px] leading-6"
                    >
                      <p>{{ p.column }}：{{ p.reason }}</p>
                      <p class="text-muted-foreground">
                        原要求：{{ p.rule_source }}
                      </p>
                    </div>
                    <p
                      v-if="materialValidation.invalid?.length"
                      class="text-[11px] text-muted-foreground"
                    >
                      未通过校验的字段已留空，原建议和来源仍保存在材料记录中。
                    </p>
                    <div
                      v-for="p in materialValidation.requires_review || []"
                      :key="p.column + p.rule_source"
                      class="rounded bg-muted/40 p-2 text-[11px] leading-6"
                    >
                      <p>{{ p.column }}：{{ p.reason }}</p>
                      <p class="text-muted-foreground">
                        原要求：{{ p.rule_source }}
                      </p>
                    </div>
                  </div>
                </template>
                <p v-else class="helper">
                  可直接使用名单开始分类；没有原始来源支持的作者、单位和出版信息保持待补充。
                </p></template
              >
            </div>
          </template>
          <div
            v-else
            class="flex min-h-96 flex-col items-center justify-center p-8 text-center"
          >
            <div
              class="mb-4 flex size-12 items-center justify-center rounded-2xl border bg-muted/30"
            >
              <BookOpen class="size-5 text-muted-foreground" />
            </div>
            <h2 class="text-sm font-medium">选择一篇论文</h2>
            <p class="mt-2 max-w-52 text-xs leading-6 text-muted-foreground">
              查看原始来源、核验分支<br />和当前可执行的下一步。
            </p>
          </div>
        </section>
      </div>
    </template>
    <div v-else-if="page === 'materials'" class="settings-grid">
      <Card class="h-fit shadow-none"
        ><CardHeader
          ><CardTitle class="flex items-center gap-2 text-base"
            ><FileSpreadsheet
              class="size-4 text-primary"
            />注册导入模板</CardTitle
          ><CardDescription class="text-xs leading-6"
            >保留原表的工作表、样式与格式。新增成果类型时可直接注册平台实际模板。</CardDescription
          ></CardHeader
        ><CardContent class="space-y-4"
          ><label class="field"
            >工作表名称<Input
              v-model="templateSheet"
              class="mt-2 text-xs"
              placeholder="留空使用第一个工作表" /></label
          ><label class="field"
            >表头行号<Input
              type="number"
              min="1"
              v-model="templateRow"
              class="mt-2 text-xs" /></label
          ><label class="field"
            >必填列名<Textarea
              v-model="required"
              class="mt-2 text-xs"
              placeholder="填写实际模板要求，用逗号或换行分隔" /></label
          ><label class="field"
            >模板要求 / 格式与枚举说明<Textarea
              v-model="templateNotes"
              class="mt-2 min-h-24 text-xs"
              placeholder="原表的表头说明会自动读取，也可在此补充平台要求" /></label
          ><Button class="w-full" :disabled="locked" @click="register"
            ><Plus />选择 Excel 并注册</Button
          ></CardContent
        ></Card
      >
      <div class="space-y-4">
        <div class="rounded-xl border border-dashed bg-muted/30 p-5">
          <p class="flex items-center gap-2 text-sm font-medium">
            <Database class="size-4 text-primary" />优先使用数据库原始导出
          </p>
          <p class="mt-2 text-xs leading-6 text-muted-foreground">
            先检查 WOS、CNKI 等数据库能否导出 Excel /
            TXT，再使用模板补齐。字段缺失会列入待补充，保留每个 AI 字段的来源。
          </p>
        </div>
        <Card v-for="s in schemas" :key="s.id" class="gap-0 py-0 shadow-none"
          ><CardContent class="p-5"
            ><div class="mb-4 flex items-center gap-3">
              <div
                class="flex size-9 items-center justify-center rounded-lg bg-emerald-500/10"
              >
                <FileSpreadsheet class="size-4 text-emerald-600" />
              </div>
              <div>
                <h3 class="text-sm font-medium">{{ s.name }}</h3>
                <p class="mt-1 text-[10px] text-muted-foreground">
                  {{ s.sheet }} · {{ s.columns.length }} 列 · 表头第
                  {{ s.header_row + 1 }} 行
                </p>
              </div>
              <Badge variant="outline" class="ml-auto text-[10px]"
                >已注册</Badge
              >
            </div>
            <p class="text-xs leading-6">
              必填：{{ s.required.join("、") || "尚未配置，请补充要求" }}
            </p>
            <p
              v-if="s.field_rules?.length"
              class="mt-2 text-[11px] text-muted-foreground"
            >
              已读取
              {{ s.field_rules.length }} 条字段规则；导出时重新核对实际模板。
            </p>
            <details v-if="s.notes">
              <summary>查看字段与格式要求</summary>
              <p
                class="mt-2 whitespace-pre-wrap text-[11px] leading-6 text-muted-foreground"
              >
                {{ s.notes }}
              </p>
            </details>
            <details>
              <summary>查看可填写列名</summary>
              <p
                class="mt-2 break-words text-[11px] leading-6 text-muted-foreground"
              >
                {{ s.columns.join("、") }}
              </p>
              <p class="mt-2 text-[11px] text-muted-foreground">
                同名列使用列位置区分，导出的 Excel 原表头保持不变。
              </p>
            </details></CardContent
          ></Card
        >
        <div
          v-if="!schemas.length"
          class="rounded-xl border bg-card px-8 py-12 text-center"
        >
          <Files class="mx-auto mb-3 size-7 text-muted-foreground/50" />
          <h3 class="text-sm font-medium">还没有注册模板</h3>
          <p class="mt-2 text-xs leading-6 text-muted-foreground">
            期刊论文、会议论文、科技论文、著作章节<br />以及其他平台模板都可以在这里管理。
          </p>
        </div>
      </div>
    </div>
    <div v-else class="settings-grid">
      <Card class="h-fit shadow-none"
        ><CardHeader
          ><CardTitle class="flex items-center gap-2 text-base"
            ><Sparkles class="size-4 text-primary" />模型 API</CardTitle
          ><CardDescription class="text-xs leading-6"
            >分类和模板填写将调用你的模型。密钥存入系统凭据，页面不回显已保存的密钥。</CardDescription
          ></CardHeader
        ><CardContent class="space-y-5"
          ><label class="field"
            >API 基础地址<Input
              v-model="api.base"
              class="mt-2 text-xs"
              placeholder="https://…/api/v1" /></label
          ><label class="field"
            >模型名称<Input v-model="api.model" class="mt-2 text-xs" /></label
          ><label class="field"
            >API 密钥<Input
              type="password"
              v-model="api.key"
              class="mt-2 text-xs"
              autocomplete="off"
              :placeholder="
                api.configured ? '已配置；留空保留现有密钥' : '填写 API 密钥'
              " /></label
          ><Button :disabled="locked" @click="saveApi"
            ><Check />保存模型设置</Button
          ></CardContent
        ></Card
      >
      <div class="space-y-4">
        <Card class="shadow-none"
          ><CardHeader
            ><CardTitle class="flex items-center gap-2 text-base"
              ><FolderOpen class="size-4 text-primary" />本地工作文件</CardTitle
            ><CardDescription class="text-xs leading-6"
              >名单任务、原始导出、模板和浏览器访问状态保存在专用目录。</CardDescription
            ></CardHeader
          ><CardContent
            ><p class="path mb-5 rounded-lg border bg-muted/30 p-3">
              {{ workspace.root || "尚未读取" }}
            </p>
            <Button variant="outline" @click="run('open_folder')"
              ><FolderOpen />打开工作目录</Button
            ></CardContent
          ></Card
        >
        <Card class="shadow-none">
          <CardHeader
            ><CardTitle class="flex items-center gap-2 text-base"
              ><Files class="size-4 text-primary" />迁移旧版记录</CardTitle
            >
            <CardDescription class="text-xs leading-6"
              >先关闭旧版助手，再选择含 list.xlsx 和 runtime
              的旧版代码目录。保留完整日志与原始
              TXT、分类结果和提交准备资料；历史建议与草稿仍需复核，已有平台操作先回读。不会读取旧版密钥或浏览器登录数据。</CardDescription
            >
          </CardHeader>
          <CardContent class="space-y-4">
            <Button variant="outline" :disabled="locked" @click="previewLegacy"
              ><FolderOpen />选择旧版目录并预览</Button
            >
            <template v-if="legacyPreview">
              <p class="path rounded-lg border bg-muted/30 p-3">
                {{ legacyPreview.root }}
              </p>
              <p class="text-xs leading-6">
                {{ legacyPreview.roster_count }} 条名单 ·
                {{ legacyPreview.journal_count }} 条进度 ·
                {{ legacyPreview.import_count }} 条导入日志。名单外的
                {{ legacyPreview.orphan_count }} 个历史 SA
                也会保留；本篇已有导入时禁止创建新批次。
              </p>
              <div
                class="max-h-60 overflow-auto rounded-lg border p-3 text-xs leading-6"
              >
                <div
                  v-for="entry in legacyPreview.entries.filter(
                    (e) => e.histories || e.materials,
                  )"
                  :key="entry.sa_id"
                  class="border-b py-2 last:border-0"
                >
                  <p>{{ entry.sa_id }} · {{ entry.title }}</p>
                  <p class="text-muted-foreground">
                    <span v-if="entry.materials"
                      >{{ entry.materials }} 份历史材料 ·
                    </span>
                    {{ entry.phases.join(" / ")
                    }}<span v-if="entry.input_changed">
                      · 输入版本不同，待核对</span
                    ><span v-if="entry.needs_readback"> · 需回读平台</span>
                  </p>
                </div>
              </div>
              <p class="text-xs leading-6">
                {{ legacyPreview.classification_count ?? 0 }} 条分类 ·
                {{ legacyPreview.prepared_count ?? 0 }} 条准备记录 ·
                {{ legacyPreview.material_file_count ?? 0 }} 个原文件。{{
                  legacyPreview.unbound_material_count ?? 0
                }}
                条未绑定资料将保存在历史归档中。
              </p>
              <details
                v-if="legacyPreview.material_warnings?.length"
                class="text-xs leading-6"
              >
                <summary>
                  查看迁移注意事项（{{
                    legacyPreview.material_warnings.length
                  }}）
                </summary>
                <p
                  v-for="(warning, index) in legacyPreview.material_warnings"
                  :key="index"
                  class="break-all"
                >
                  {{ warning }}
                </p>
              </details>
              <Button :disabled="locked" @click="migrateLegacy"
                ><Check />迁移这些记录</Button
              >
            </template>
          </CardContent>
        </Card>
        <LegacyMaterials :batches="workspace.legacy_materials || []" />
        <div class="rounded-xl border border-dashed p-5">
          <p class="flex items-center gap-2 text-xs font-medium">
            <ShieldCheck class="size-4 text-primary" />进度会保留
          </p>
          <p class="mt-2 text-xs leading-6 text-muted-foreground">
            暂停或重新打开工作台可以继续任务。已经核验的文件会复用，未确认的平台写入需要先回读。
          </p>
        </div>
      </div>
    </div>
  </WorkbenchShell>
  <Dialog
    :open="!!confirmation"
    @update:open="
      (value) => {
        if (!value) settleConfirmation(false);
      }
    "
    ><DialogContent class="max-w-md"
      ><DialogHeader
        ><DialogTitle class="flex items-center gap-2"
          ><ShieldCheck class="size-5 text-primary" />{{
            confirmation?.title
          }}</DialogTitle
        ><DialogDescription
          class="pt-3 whitespace-pre-wrap text-xs leading-7"
          >{{ confirmation?.description }}</DialogDescription
        ></DialogHeader
      ><DialogFooter class="mt-3 gap-2"
        ><Button variant="outline" @click="settleConfirmation(false)"
          >返回核对</Button
        ><Button @click="settleConfirmation(true)"
          >确认执行</Button
        ></DialogFooter
      ></DialogContent
    ></Dialog
  >
</template>
