<script setup lang="ts">
import { computed, ref } from "vue";
import {
  ArrowRight,
  BookOpen,
  Database,
  FileSpreadsheet,
  FolderOpen,
  Layers3,
  Link2,
  Settings2,
  ShieldCheck,
} from "@lucide/vue";
import type { Workspace } from "@/types";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";

const props = defineProps<{
  workspace: Workspace;
  configured: boolean;
  templates: number;
  locked: boolean;
}>();
const emit = defineEmits<{
  action: [
    action:
      | "import"
      | "tasks"
      | "review"
      | "materials"
      | "settings"
      | "report"
      | "folder",
  ];
  browser: [role: string, entry?: string];
}>();
const tab = ref("flow");
const manifest = computed(() => props.workspace.framework);
const summary = computed(() => {
  const tasks = props.workspace.tasks;
  return {
    total: tasks.length,
    zero: tasks.filter(
      (t) => t.record.matches === 0 && !t.record.done && !t.record.skipped,
    ).length,
    unknown: tasks.filter((t) => t.stage === "unknown").length,
    completed: tasks.filter((t) => t.stage === "completed" && !t.pending_input)
      .length,
  };
});
const workflow = [
  {
    id: "import",
    label: "导入名单",
    detail: "Excel 与负责人",
    icon: FileSpreadsheet,
  },
  {
    id: "tasks",
    label: "核对与查库",
    detail: "按匹配数量分流",
    icon: ShieldCheck,
  },
  {
    id: "materials",
    label: "准备材料",
    detail: "原始导出优先",
    icon: Database,
  },
  { id: "tasks", label: "执行与回读", detail: "分阶段确认结果", icon: Link2 },
  {
    id: "report",
    label: "导出结果",
    detail: "完整来源与记录",
    icon: FolderOpen,
  },
] as const;
const browserLabels: Record<string, string> = {
  wos: "WOS 数据库",
  sa: "SA 比对",
  library: "机构库前端",
  import: "导入与批次",
  scholar: "学者与别名",
  duplicate: "重复数据管理",
};
function browserState(role: string) {
  const state = props.workspace.browsers[role];
  return !state?.open
    ? "未打开"
    : state.usable
      ? "工作页已打开"
      : "访问或登录页面";
}
</script>

<template>
  <div class="space-y-5">
    <section class="rounded-2xl border bg-card p-5 lg:p-6">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p
            class="mb-2 flex items-center gap-2 text-xs font-medium text-primary"
          >
            <Layers3 class="size-4" />统一流程
          </p>
          <h2 class="text-lg font-semibold">从名单到处理结果，一个工作空间</h2>
          <p class="mt-2 max-w-2xl text-xs leading-6 text-muted-foreground">
            资料、核验依据与执行记录跟随每条 SA
            任务保存。选择下方入口进入实际操作，结果不明的任务先回读确认。
          </p>
        </div>
        <Badge variant="outline" class="text-[10px]">{{
          manifest?.live_verified ? "已有真实验收记录" : "真实平台待验收"
        }}</Badge>
      </div>
      <div class="mt-5 grid gap-2 sm:grid-cols-2 min-[1150px]:grid-cols-5">
        <Button
          v-for="(item, index) in workflow"
          :key="index"
          variant="outline"
          class="h-auto justify-start gap-3 whitespace-normal p-3 text-left"
          :disabled="locked"
          @click="emit('action', item.id)"
        >
          <span
            class="flex size-8 shrink-0 items-center justify-center rounded-lg bg-primary/8 text-primary"
            ><component :is="item.icon" class="size-4"
          /></span>
          <span class="min-w-0"
            ><span class="block text-xs">{{ index + 1 }}. {{ item.label }}</span
            ><span
              class="mt-1 block text-[10px] font-normal text-muted-foreground"
              >{{ item.detail }}</span
            ></span
          >
        </Button>
      </div>
      <div
        class="mt-4 flex flex-wrap gap-x-5 gap-y-2 text-xs text-muted-foreground"
      >
        <span
          >名单
          <strong class="text-foreground">{{ summary.total }}</strong> 条</span
        >
        <span
          >零匹配待办
          <strong class="text-foreground">{{ summary.zero }}</strong> 条</span
        >
        <span
          >SA 完成
          <strong class="text-foreground">{{ summary.completed }}</strong>
          条</span
        >
        <Button
          v-if="summary.unknown"
          variant="link"
          size="sm"
          class="h-auto p-0 text-amber-700 dark:text-amber-400"
          @click="emit('action', 'review')"
          >{{ summary.unknown }} 条结果待确认 <ArrowRight class="size-3"
        /></Button>
      </div>
    </section>

    <div class="grid gap-5 lg:grid-cols-[minmax(0,1fr)_19rem]">
      <div class="min-w-0 space-y-4">
        <Tabs v-model="tab"
          ><TabsList class="h-9"
            ><TabsTrigger value="flow">处理分支</TabsTrigger
            ><TabsTrigger value="channels">来源与渠道</TabsTrigger
            ><TabsTrigger value="services">功能模块</TabsTrigger></TabsList
          ></Tabs
        >
        <p
          v-if="!manifest"
          class="rounded-xl border border-dashed p-5 text-xs text-muted-foreground"
        >
          正在读取工作台框架。若未加载，请刷新工作台并确认桌面服务已启动。
        </p>
        <template v-else-if="tab === 'flow'">
          <Card
            v-for="flow in manifest.flows"
            :key="flow.id"
            class="gap-0 py-0 shadow-none"
            ><CardContent class="p-5">
              <div class="mb-3 flex items-center justify-between">
                <h3 class="text-sm font-semibold">{{ flow.label }}</h3>
                <Badge variant="secondary" class="text-[10px]">{{
                  flow.id === "zero"
                    ? "先确定原因"
                    : flow.id === "existing"
                      ? "逐项核验"
                      : "同篇依据与主条目"
                }}</Badge>
              </div>
              <ol
                class="flex flex-wrap gap-y-2 text-xs leading-6 text-muted-foreground"
              >
                <li
                  v-for="(step, index) in flow.steps"
                  :key="step"
                  class="flex items-center"
                >
                  {{ step
                  }}<ArrowRight
                    v-if="index < flow.steps.length - 1"
                    class="mx-2 size-3 shrink-0 text-muted-foreground/50"
                  />
                </li>
              </ol>
              <ul class="mt-3 space-y-1 border-t pt-3 text-[11px] leading-6">
                <li v-for="outcome in flow.outcomes" :key="outcome">
                  {{ outcome }}
                </li>
              </ul>
            </CardContent></Card
          >
          <Button
            variant="outline"
            class="w-full"
            @click="emit('action', 'tasks')"
            >进入任务工作台 <ArrowRight
          /></Button>
        </template>
        <template v-else-if="tab === 'channels'">
          <div
            class="rounded-xl border bg-muted/30 p-4 text-xs leading-6 text-muted-foreground"
          >
            优先获取数据库原始文件。渠道登记、原生导出、文件解析与平台提交分别接入；登记存在并不代表已可自动下载或导入。
          </div>
          <div class="grid gap-3 xl:grid-cols-2">
            <Card
              v-for="channel in manifest.channels"
              :key="channel.id"
              class="gap-0 py-0 shadow-none"
              ><CardContent class="p-4">
                <div class="flex items-start justify-between gap-3">
                  <h3 class="text-xs font-medium">{{ channel.label }}</h3>
                  <Badge variant="outline" class="shrink-0 text-[10px]">{{
                    channel.automated
                      ? "已接入 · 待现场验收"
                      : channel.capabilities.template === "implemented"
                        ? "使用实际模板"
                        : "渠道已登记"
                  }}</Badge>
                </div>
                <p class="mt-2 text-[10px] text-muted-foreground">
                  登记格式：{{ channel.formats.join(" / ").toUpperCase() }}
                </p>
                <div class="mt-3 grid grid-cols-2 gap-x-3 gap-y-2 text-[11px]">
                  <span
                    v-for="[key, label] in [
                      ['export', '原生导出'],
                      ['parse', '文件解析'],
                      ['submit', '自动提交'],
                      ['template', '模板准备'],
                    ]"
                    :key="key"
                    class="flex justify-between gap-2"
                    ><span class="text-muted-foreground">{{ label }}</span
                    ><span>{{
                      channel.capabilities[key] === "implemented"
                        ? "已接入"
                        : channel.capabilities[key] === "registered_only"
                          ? "需注册模板"
                          : "待接入"
                    }}</span></span
                  >
                </div>
                <Button
                  v-if="channel.id === 'wos_txt'"
                  variant="outline"
                  size="sm"
                  class="mt-4 w-full"
                  :disabled="locked"
                  @click="emit('browser', 'wos')"
                  >打开 WOS 工作页</Button
                >
                <Button
                  v-else-if="channel.capabilities.template === 'implemented'"
                  variant="outline"
                  size="sm"
                  class="mt-4 w-full"
                  @click="emit('action', 'materials')"
                  >管理实际模板</Button
                >
              </CardContent></Card
            >
          </div>
        </template>
        <div v-else class="grid gap-3 xl:grid-cols-2">
          <Card
            v-for="service in manifest.services"
            :key="service.id"
            class="gap-0 py-0 shadow-none"
            ><CardContent class="p-4"
              ><div class="flex items-center justify-between gap-3">
                <h3 class="text-xs font-medium">{{ service.label }}</h3>
                <Badge variant="outline" class="text-[10px]">{{
                  service.state === "implemented" ? "已接入" : "部分接入"
                }}</Badge>
              </div>
              <p class="mt-2 text-xs leading-6 text-muted-foreground">
                {{ service.description }}
              </p>
              <p
                class="mt-3 border-t pt-2 text-[11px] leading-5 text-muted-foreground"
              >
                {{ service.limitation }}
              </p></CardContent
            ></Card
          >
        </div>
      </div>
      <aside class="space-y-4">
        <Card class="gap-0 shadow-none"
          ><CardHeader class="pb-3"
            ><CardTitle class="flex items-center gap-2 text-sm"
              ><BookOpen class="size-4 text-primary" />浏览器与访问</CardTitle
            ><CardDescription class="text-xs leading-6"
              >登录在各自窗口内完成。工作页打开状态不等于已验证访问权限。</CardDescription
            ></CardHeader
          ><CardContent class="space-y-2">
            <Button
              variant="outline"
              class="h-auto w-full justify-start py-3 text-xs"
              :disabled="locked"
              @click="emit('browser', 'wos', 'database_directory')"
              ><Database class="size-4" />图书馆数据库入口</Button
            >
            <Button
              v-for="browser in manifest?.browsers || []"
              :key="browser.id"
              variant="ghost"
              class="h-auto w-full justify-between gap-2 py-2.5"
              :disabled="locked"
              @click="emit('browser', browser.id)"
              ><span class="text-xs">{{
                browserLabels[browser.id] || browser.label
              }}</span
              ><span class="text-[10px] font-normal text-muted-foreground">{{
                browserState(browser.id)
              }}</span></Button
            >
          </CardContent></Card
        >
        <Card class="gap-0 shadow-none"
          ><CardContent class="space-y-3 p-5"
            ><div class="flex items-center justify-between text-xs">
              <span>AI 服务</span
              ><Badge variant="outline" class="text-[10px]">{{
                configured ? "密钥已配置" : "未配置"
              }}</Badge>
            </div>
            <div class="flex items-center justify-between text-xs">
              <span>已注册模板</span
              ><span class="tabular-nums">{{ templates }}</span>
            </div>
            <Button
              variant="outline"
              size="sm"
              class="w-full"
              @click="emit('action', 'settings')"
              ><Settings2 />模型与设置</Button
            ><Button
              variant="ghost"
              size="sm"
              class="w-full"
              @click="emit('action', 'folder')"
              ><FolderOpen />打开工作文件</Button
            ></CardContent
          ></Card
        >
        <div class="rounded-xl border border-dashed p-4">
          <h3 class="mb-2 text-xs font-medium">执行规则</h3>
          <ul class="space-y-2 text-[11px] leading-5 text-muted-foreground">
            <li v-for="rule in manifest?.rules || []" :key="rule">
              {{ rule }}
            </li>
          </ul>
        </div>
      </aside>
    </div>
  </div>
</template>
