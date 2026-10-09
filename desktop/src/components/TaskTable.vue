<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  ArrowUpDown,
  ChevronLeft,
  ChevronRight,
  FileText,
  SearchX,
  BookOpen,
  Upload,
} from "@lucide/vue";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { Task } from "@/types";
const props = defineProps<{
  tasks: Task[];
  selected: string;
  stageNames: Record<string, string>;
  hasRoster: boolean;
}>();
const emit = defineEmits<{ select: [id: string]; import: []; clear: [] }>();
const page = ref(0),
  sort = ref(""),
  ascending = ref(true);
watch(
  () => props.tasks,
  () => (page.value = 0),
);
const sorted = computed(() => {
  const rows = [...props.tasks];
  if (sort.value)
    rows.sort(
      (a, b) =>
        (sort.value === "title"
          ? a.record.title.localeCompare(b.record.title, "zh")
          : a.record.matches - b.record.matches) * (ascending.value ? 1 : -1),
    );
  return rows;
});
const pages = computed(() => Math.max(1, Math.ceil(sorted.value.length / 12)));
const visible = computed(() =>
  sorted.value.slice(page.value * 12, page.value * 12 + 12),
);
function order(key: string) {
  ascending.value = sort.value === key ? !ascending.value : true;
  sort.value = key;
  page.value = 0;
}
</script>
<template>
  <section class="task-table-panel overflow-hidden rounded-xl border bg-card">
    <div class="flex h-13 items-center gap-2 border-b px-4">
      <FileText class="size-4 text-muted-foreground" />
      <h3 class="text-sm font-semibold">论文任务</h3>
      <Badge variant="secondary" class="ml-auto text-[10px]"
        >{{ tasks.length }} 项</Badge
      >
    </div>
    <div v-if="tasks.length" class="table-body-scroll">
      <Table class="table-fixed"
        ><TableHeader
          ><TableRow class="hover:bg-transparent"
            ><TableHead class="w-[63%] pl-4"
              ><button
                class="inline-flex items-center gap-1.5"
                aria-label="按题名排序"
                @click="order('title')"
              >
                论文与来源<ArrowUpDown class="size-3" /></button></TableHead
            ><TableHead class="w-[13%]"
              ><button
                class="inline-flex items-center gap-1"
                aria-label="按匹配数排序"
                @click="order('matches')"
              >
                匹配<ArrowUpDown class="size-3" /></button></TableHead
            ><TableHead>当前阶段</TableHead></TableRow
          ></TableHeader
        ><TableBody
          ><TableRow
            v-for="t in visible"
            :key="t.id"
            class="cursor-pointer"
            :data-state="selected === t.id ? 'selected' : undefined"
            tabindex="0"
            :aria-label="t.record.title"
            @click="emit('select', t.id)"
            @keydown.enter="emit('select', t.id)"
            ><TableCell class="relative py-4 pl-4"
              ><span
                v-if="selected === t.id"
                class="absolute inset-y-2 left-0 w-0.5 rounded bg-primary"
              ></span>
              <p class="line-clamp-2 text-xs font-medium leading-5">
                {{ t.record.title }}
              </p>
              <p class="mt-1.5 text-[10px] text-muted-foreground">
                {{ t.record.owner }} · 第 {{ t.record.row }} 行<span
                  v-if="t.record.skipped"
                >
                  · 跳过项</span
                >
              </p>
              <p
                class="mt-1 truncate font-mono text-[10px] text-muted-foreground/80"
              >
                {{ t.record.doi || t.id }}
              </p></TableCell
            ><TableCell
              ><Badge
                variant="outline"
                class="rounded-md px-2 font-mono text-[11px]"
                >{{ t.record.matches }}</Badge
              ></TableCell
            ><TableCell
              ><span
                class="status-badge"
                :class="
                  t.stage === 'completed'
                    ? 'status-done'
                    : t.last_error || t.stage === 'unknown'
                      ? 'status-review'
                      : 'status-neutral'
                "
                ><span class="size-1 shrink-0 rounded-full bg-current"></span
                >{{ stageNames[t.stage] || t.stage }}</span
              ></TableCell
            ></TableRow
          ></TableBody
        ></Table
      >
    </div>
    <div
      v-else
      class="flex min-h-80 flex-col items-center justify-center px-6 py-12 text-center"
    >
      <div
        class="mb-4 flex size-12 items-center justify-center rounded-2xl border bg-muted/40"
      >
        <SearchX
          v-if="hasRoster"
          class="size-5 text-muted-foreground"
        /><BookOpen v-else class="size-5 text-primary" />
      </div>
      <h3 class="text-sm font-medium">
        {{ hasRoster ? "没有符合筛选的任务" : "从论文名单开始" }}
      </h3>
      <p class="mb-5 mt-2 max-w-60 text-xs leading-6 text-muted-foreground">
        {{
          hasRoster
            ? "调整负责人、搜索词或筛选条件。"
            : "导入 list.xlsx，自动按匹配数量分流。每一篇的来源与处理进度都会保留。"
        }}
      </p>
      <Button v-if="!hasRoster" size="sm" @click="emit('import')"
        ><Upload />选择名单文件</Button
      ><Button v-else variant="outline" size="sm" @click="emit('clear')"
        >清除筛选</Button
      >
    </div>
    <footer
      v-if="tasks.length"
      class="flex h-12 items-center justify-between gap-2 border-t px-4"
    >
      <span class="text-[10px] text-muted-foreground"
        >第 {{ page + 1 }} / {{ pages }} 页 · 每页 12 项</span
      >
      <div class="flex gap-1">
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label="上一页"
          :disabled="page === 0"
          @click="page--"
          ><ChevronLeft /></Button
        ><Button
          variant="ghost"
          size="icon-xs"
          aria-label="下一页"
          :disabled="page + 1 >= pages"
          @click="page++"
          ><ChevronRight
        /></Button>
      </div>
    </footer>
  </section>
</template>
