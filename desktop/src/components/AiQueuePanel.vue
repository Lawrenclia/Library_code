<script setup lang="ts">
import { computed, ref } from "vue";
import type { Workspace, Template } from "@/types";
import type { DesktopCommand } from "@/services/desktop";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Sparkles, Pause } from "@lucide/vue";
const props = defineProps<{
  workspace: Workspace;
  owner: string;
  locked: boolean;
  configured: boolean;
  templates: Template[];
  run: <T>(
    command: DesktopCommand,
    args?: Record<string, unknown>,
    success?: string,
  ) => Promise<T | undefined>;
}>();
const zeroOnly = ref(true),
  templateId = ref("");
const expanded = ref(false);
const queue = computed(() => props.workspace.ai_queue);
const unfinished = computed(
  () => queue.value && !["completed", "cancelled"].includes(queue.value.status),
);
const states: Record<string, string> = {
  running: "处理中",
  paused: "已暂停",
  blocked: "API 或模板受阻",
  interrupted: "重启后待继续",
  completed: "本轮已结束",
  cancelled: "原范围已结束",
};
const results: Record<string, string> = {
  classified: "建议已保存",
  reused: "复用同版本建议",
  failed: "未获得有效建议",
  not_executed: "未请求模型",
  unconfirmed: "请求结果待核对，未重发",
};
async function start() {
  await props.run(
    "run_ai_queue",
    {
      owner: props.owner,
      zeroOnly: zeroOnly.value,
      templateId: templateId.value || null,
    },
    "AI 批量处理记录已保存，请核对逐篇结果。",
  );
}
</script>
<template>
  <section
    aria-label="AI 批量处理"
    class="mb-4 rounded-xl border bg-card p-4 text-xs"
  >
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <p class="flex items-center gap-2 font-medium">
          <Sparkles class="size-4 text-primary" />批量 AI 分类与填写建议
        </p>
        <p class="mt-2 text-[11px] leading-6 text-muted-foreground">
          按负责人固定原名单，使用已配置的 API
          额度。普通逐篇失败继续；来源、名单或模型变化会记录原因。仅保存建议，导出模板材料仍需逐篇校验。
        </p>
      </div>
      <Button
        v-if="!unfinished"
        size="sm"
        variant="outline"
        @click="expanded = !expanded"
      >
        {{ expanded ? "收起配置" : "展开批量 AI 配置" }}
      </Button>
    </div>
    <div
      v-if="expanded && !unfinished"
      class="mt-3 flex flex-wrap items-end gap-3"
    >
      <label class="text-[11px]"
        >处理范围<select
          v-model="zeroOnly"
          aria-label="AI 批量范围"
          :disabled="locked || !!unfinished"
          class="native-select mt-1 block"
        >
          <option :value="true">仅零匹配论文</option>
          <option :value="false">当前负责人全部待处理论文</option>
        </select></label
      >
      <label class="min-w-40 flex-1 text-[11px]"
        >批量使用模板<select
          v-model="templateId"
          aria-label="AI 批量模板"
          :disabled="locked || !!unfinished"
          class="native-select mt-1 block w-full"
        >
          <option value="">仅分类与渠道建议</option>
          <option
            v-for="template in templates"
            :key="template.id"
            :value="template.id"
          >
            {{ template.name }}
          </option>
        </select></label
      >
      <Button
        size="sm"
        :disabled="locked || !owner || !configured || !!unfinished"
        @click="start"
        ><Sparkles />开始批量 AI</Button
      >
    </div>
    <p
      v-if="expanded && !unfinished && !owner"
      class="mt-2 text-[11px] text-muted-foreground"
    >
      先选择负责人。
    </p>
    <p
      v-if="expanded && !unfinished && !configured"
      class="mt-2 text-[11px] text-muted-foreground"
    >
      先在模型与设置中配置 API。
    </p>
    <div v-if="queue" class="mt-4 border-t pt-3">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <p>
          {{ states[queue.status] || queue.status }} · 原范围：{{
            queue.owner
          }}
          · {{ queue.zero_only ? "零匹配" : "全部待处理" }} · 已记录
          {{ queue.cursor }} / {{ queue.targets.length }} 篇
        </p>
        <div class="flex gap-2">
          <Button
            v-if="
              queue.status === 'running' && workspace.running_service === 'ai'
            "
            size="sm"
            variant="outline"
            @click="
              run(
                'pause_ai_queue',
                {},
                '已请求暂停，当前 AI 请求结束后保存进度。',
              )
            "
            ><Pause />暂停 AI 后续任务</Button
          >
          <template v-else-if="unfinished"
            ><Button
              size="sm"
              :disabled="locked"
              @click="
                run(
                  'resume_ai_queue',
                  { id: queue.id },
                  '原 AI 范围的处理记录已保存。',
                )
              "
              >核对并继续原 AI 范围</Button
            ><Button
              v-if="queue.status !== 'running'"
              size="sm"
              variant="outline"
              :disabled="locked"
              @click="
                run(
                  'cancel_ai_queue',
                  { id: queue.id },
                  '已结束原 AI 范围，完整记录仍保留。',
                )
              "
              >结束原 AI 范围</Button
            ></template
          >
        </div>
      </div>
      <p class="mt-2 text-[11px] text-muted-foreground">
        模型：{{ queue.config.model }} · 模板：{{
          queue.template?.name || "仅分类"
        }}。继续只处理原范围剩余项，失败或中断而没有结果的请求不自动重试，可在单篇页核对后补做。
      </p>
      <Progress
        class="mt-3 h-1.5"
        :model-value="
          queue.targets.length ? (queue.cursor / queue.targets.length) * 100 : 0
        "
      />
      <p
        v-if="queue.pause_requested && queue.status === 'running'"
        class="mt-2 text-muted-foreground"
      >
        已请求暂停，等待当前 AI 请求结束。
      </p>
      <p
        v-if="queue.last_error"
        class="mt-2 text-amber-700 dark:text-amber-400"
      >
        {{ queue.last_error.message }}
      </p>
      <details v-if="queue.outcomes.length" class="mt-2">
        <summary class="cursor-pointer">查看 AI 逐篇结果</summary>
        <ul class="mt-2 max-h-44 space-y-2 overflow-auto">
          <li v-for="result in queue.outcomes" :key="result.id">
            SA {{ result.id }} · {{ results[result.status] || result.status
            }}<span v-if="result.error"> · {{ result.error.message }}</span>
          </li>
        </ul>
      </details>
    </div>
  </section>
</template>
