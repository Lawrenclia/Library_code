<script setup lang="ts">
import { computed, reactive, watch } from "vue";
import type { Task, Evidence } from "@/types";
import type { DesktopCommand } from "@/services/desktop";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
const props = defineProps<{
  task: Task;
  locked: boolean;
  channels: { id: string; label: string }[];
  run: <T>(
    command: DesktopCommand,
    args?: Record<string, unknown>,
    success?: string,
  ) => Promise<T | undefined>;
}>();
const localTime = () => {
  const date = new Date();
  return new Date(date.getTime() - date.getTimezoneOffset() * 60_000)
    .toISOString()
    .slice(0, 16);
};
const form = reactive({
  channel: "wos_txt",
  source_url: "",
  scope: "",
  query: "",
  field: "title",
  time: localTime(),
  outcome: "zero_results",
  total: "0",
  explanation: "",
});
watch(
  () => props.task.id,
  () => {
    Object.assign(form, {
      channel: "wos_txt",
      source_url: "",
      scope: "",
      query: props.task.record.title,
      field: "title",
      time: localTime(),
      outcome: "zero_results",
      total: "0",
      explanation: "",
    });
  },
  { immediate: true },
);
interface Observation {
  channel: string;
  source_url: string;
  scope: string;
  query: string;
  field: string;
  observed_at: number;
  outcome: string;
  total: number | null;
  explanation: string;
}
const records = computed(() =>
  props.task.evidence
    .filter((e) => e.kind === "search_scope")
    .map((e) => {
      try {
        const receipt = JSON.parse(e.text) as {
          schema: string;
          task_id: string;
          input_hash: string;
          reported_by: string;
          observation: Observation;
        };
        const observation = receipt.observation;
        if (
          receipt.schema !== "source_search_scope_v1" ||
          receipt.task_id !== props.task.id ||
          receipt.reported_by !== "human" ||
          !observation ||
          typeof observation.query !== "string" ||
          typeof observation.scope !== "string" ||
          typeof observation.explanation !== "string" ||
          !Number.isSafeInteger(observation.observed_at)
        )
          return null;
        return {
          id: e.id,
          current: receipt.input_hash === props.task.input_hash,
          observation,
        };
      } catch {
        return null;
      }
    })
    .filter((r): r is NonNullable<typeof r> => r !== null)
    .reverse(),
);
const label = (outcome: string) =>
  ({
    zero_results: "明确零结果",
    unresolved_candidates: "候选待核对",
    blocked: "访问或检索受阻",
  })[outcome as "zero_results" | "unresolved_candidates" | "blocked"] ||
  outcome;
async function save() {
  const result = await props.run<Evidence>(
    "record_search_scope",
    {
      id: props.task.id,
      revision: props.task.revision,
      inputHash: props.task.input_hash,
      observation: {
        channel: form.channel,
        source_url: form.source_url,
        scope: form.scope,
        query: form.query,
        field: form.field,
        observed_at: new Date(form.time).getTime(),
        outcome: form.outcome,
        total:
          form.outcome === "blocked"
            ? null
            : form.outcome === "zero_results"
              ? 0
              : Number(form.total),
        explanation: form.explanation,
      },
    },
    "实际检索记录已保存；平台状态保持不变。",
  );
  if (result) form.explanation = "";
}
</script>
<template>
  <section
    class="space-y-3 rounded-xl border bg-muted/20 p-4"
    data-testid="search-scope-panel"
  >
    <div>
      <p class="text-xs font-semibold">实际检索范围</p>
      <p class="helper mt-1">
        记录你已完成的查询。零结果只代表本次范围未找到；候选待核对和访问失败分别保留，不能证明论文不存在。
      </p>
    </div>
    <details class="space-y-3" :open="!records.length">
      <summary class="cursor-pointer text-xs text-primary">
        新增检索记录
      </summary>
      <div class="mt-3 space-y-3">
        <label class="field"
          >数据库 / 来源<select
            v-model="form.channel"
            class="native-select mt-2 w-full"
            aria-label="检索来源"
          >
            <option value="library">机构库前端</option>
            <option
              v-for="channel in channels"
              :key="channel.id"
              :value="channel.id"
            >
              {{ channel.label }}
            </option>
          </select></label
        >
        <label class="field"
          >实际结果页网址<Input
            v-model="form.source_url"
            class="mt-2 text-xs"
            placeholder="https://…"
        /></label>
        <label class="field"
          >检索范围<Input
            v-model="form.scope"
            class="mt-2 text-xs"
            placeholder="数据库合集、年份、语种或其他实际筛选条件"
        /></label>
        <div class="grid gap-3 sm:grid-cols-2">
          <label class="field"
            >查询字段<select
              v-model="form.field"
              class="native-select mt-2 w-full"
            >
              <option value="title">题名</option>
              <option value="doi">DOI</option>
              <option value="wos">WOS ID</option>
              <option value="keywords">关键词</option>
            </select></label
          >
          <label class="field"
            >实际查询时间<Input
              v-model="form.time"
              type="datetime-local"
              class="mt-2 text-xs"
          /></label>
        </div>
        <label class="field"
          >实际查询词<Textarea v-model="form.query" class="mt-2 text-xs"
        /></label>
        <label class="field"
          >查询结果<select
            v-model="form.outcome"
            class="native-select mt-2 w-full"
          >
            <option value="zero_results">页面明确返回 0 条</option>
            <option value="unresolved_candidates">有候选，尚未核对清楚</option>
            <option value="blocked">访问或检索受阻，未取得结果</option>
          </select></label
        >
        <label v-if="form.outcome === 'unresolved_candidates'" class="field"
          >实际候选总数<Input
            v-model="form.total"
            type="number"
            min="1"
            class="mt-2 text-xs"
        /></label>
        <label class="field"
          >结果说明 / 核对依据<Textarea
            v-model="form.explanation"
            class="mt-2 text-xs"
            placeholder="页面显示的结果、候选尚未确定的原因，或具体访问问题"
        /></label>
        <Button
          class="w-full"
          size="sm"
          variant="outline"
          :disabled="
            locked ||
            ['unknown', 'completed'].includes(task.stage) ||
            !form.source_url.trim() ||
            !form.scope.trim() ||
            !form.query.trim() ||
            !form.explanation.trim() ||
            !Number.isFinite(new Date(form.time).getTime())
          "
          @click="save"
          >保存本次检索记录</Button
        >
      </div>
    </details>
    <div
      v-for="record in records"
      :key="record.id"
      class="space-y-1 rounded-lg border bg-background p-3 text-xs"
    >
      <p class="font-medium">
        {{ label(record.observation.outcome)
        }}<span v-if="!record.current" class="ml-2 text-muted-foreground"
          >历史名单</span
        >
      </p>
      <p class="text-muted-foreground">
        {{ new Date(record.observation.observed_at).toLocaleString() }} ·
        人工记录<span v-if="record.observation.total !== null">
          · {{ record.observation.total }} 条</span
        >
      </p>
      <p class="break-words">
        {{ record.observation.scope }} · {{ record.observation.field }}：{{
          record.observation.query
        }}
      </p>
      <p class="break-all text-muted-foreground">
        {{ record.observation.source_url }}
      </p>
      <p class="whitespace-pre-wrap break-words">
        {{ record.observation.explanation }}
      </p>
    </div>
    <p class="helper">
      “未查询到文献”须有当前名单的明确零结果记录，并继续核对未确定的候选。这里只保存本地记录；平台独立备注入口仍待接通，不会设置已处理。
    </p>
  </section>
</template>
