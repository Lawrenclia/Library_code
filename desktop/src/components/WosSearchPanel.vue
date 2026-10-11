<script setup lang="ts">
import { computed } from "vue";
import type { Task, WosSearchTrace } from "@/types";
const props = defineProps<{ task: Task }>();
const searches = computed(() => {
  const grouped = new Map<string, WosSearchTrace[]>();
  for (const event of props.task.wos_searches || []) {
    const list = grouped.get(event.trace_id) || [];
    list.push(event);
    grouped.set(event.trace_id, list);
  }
  return [...grouped.entries()].reverse().map(([id, events]) => {
    const last = events[events.length - 1]!;
    const context = [...events].reverse().find((e) => e.phase === "prepared")
      ?.data.search_context;
    const labels: Record<string, string> = {
      started: "准备开始检索，尚无页面查询记录",
      prepared: "查询已准备，尚未记录结果",
      dispatch_requested: "已请求检索，结果待确认",
      observed_result: "已观察到页面结果，下载另行确认",
    };
    const outcome =
      last.phase === "finished"
        ? last.data.outcome === "record_located"
          ? "文献页已定位，下载另行确认"
          : last.data.error?.code === "NO_RESULT"
            ? "本次检索未定位文献，仍需核验"
            : last.data.error?.code === "PAUSED"
              ? "本次检索已暂停，原任务待继续"
              : "本次检索失败"
        : labels[last.phase] || "结果尚未确认";
    const source = [...events].reverse().find((e) => e.source_url)?.source_url;
    return { id, events, last, context, outcome, source };
  });
});
</script>
<template>
  <section
    v-if="searches.length"
    class="space-y-3 rounded-xl border p-4 text-xs"
  >
    <p class="font-medium">WOS 检索记录</p>
    <p class="leading-6 text-muted-foreground">
      保留页面回读的查询和结果。文件下载、论文身份和业务结论分别核验。
    </p>
    <details
      v-for="search in searches"
      :key="search.id"
      class="border-t pt-3"
      :open="search.id === searches[0]?.id"
    >
      <summary class="cursor-pointer leading-6">
        {{ search.outcome }} ·
        {{ new Date(search.last.observed_at).toLocaleString() }}
      </summary>
      <div class="mt-2 space-y-2 leading-6 text-muted-foreground">
        <p
          v-if="
            search.last.input_hash !== task.input_hash ||
            !search.last.task_snapshot_matches
          "
        >
          对应历史名单或执行时任务已变化，原记录保留。
        </p>
        <p v-if="search.context" class="break-all">
          页面回读：{{ search.context.field_label }} ·
          {{ search.context.query }}
        </p>
        <p v-else>尚未回读到实际查询字段和输入词。</p>
        <p v-if="search.context?.scope_controls?.length" class="break-all">
          范围控件记录：{{ search.context.scope_controls.join("；") }}
        </p>
        <p>范围选项记录不代表已覆盖全部数据库、索引或年份。</p>
        <p v-if="search.source" class="break-all">
          {{ search.source }}
        </p>
        <p
          v-if="search.last.data.error"
          class="text-amber-700 dark:text-amber-400"
        >
          {{ search.last.data.error.message }}
        </p>
        <details>
          <summary class="cursor-pointer">查看页面记录</summary>
          <pre
            v-for="event in search.events"
            :key="event.event_id"
            class="mt-2 max-h-48 overflow-auto whitespace-pre-wrap break-all rounded bg-muted/30 p-2 text-[10px]"
            >{{
              JSON.stringify(
                {
                  phase: event.phase,
                  source: event.source_url,
                  observed_at: event.observed_at,
                  data: event.data,
                },
                null,
                2,
              )
            }}</pre>
        </details>
      </div>
    </details>
  </section>
</template>
