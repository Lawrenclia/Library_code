<script setup lang="ts">
import { computed } from "vue";
import type { Task, Evidence } from "@/types";
import type { DesktopCommand } from "@/services/desktop";
import { Button } from "@/components/ui/button";
const props = defineProps<{
  task: Task;
  locked: boolean;
  run: <T>(
    command: DesktopCommand,
    args?: Record<string, unknown>,
    success?: string,
  ) => Promise<T | undefined>;
}>();
interface Receipt {
  schema: string;
  request: { task_id: string; input_hash: string; doi: string; url: string };
  archive_path: string;
  sha256: string;
  fetched_at: number;
  matches_input_title: boolean;
  missing: string[];
  metadata: { title: string[]; type?: string } & Record<string, unknown>;
}
const current = computed(() => {
  for (const evidence of [...props.task.evidence].reverse()) {
    if (evidence.kind !== "doi_metadata") continue;
    try {
      const receipt = JSON.parse(evidence.text) as Receipt;
      if (
        receipt.schema === "crossref_source_v1" &&
        receipt.request?.task_id === props.task.id &&
        receipt.request.input_hash === props.task.input_hash &&
        Array.isArray(receipt.metadata?.title) &&
        receipt.metadata.title.every((t) => typeof t === "string") &&
        Array.isArray(receipt.missing) &&
        receipt.missing.every((v) => typeof v === "string")
      )
        return { evidence, receipt };
    } catch {
      /* The backend reports damaged sources when they are used. */
    }
  }
  return null;
});
async function lookup(refresh = false) {
  await props.run<{ evidence: Evidence; reused: boolean }>(
    "lookup_doi_source",
    { id: props.task.id, refresh },
    "DOI 来源已保存，可用于分类和模板填写；归属和本库状态仍须单独核对。",
  );
}
</script>
<template>
  <div class="space-y-2 rounded-lg border p-3" data-testid="doi-source">
    <p class="text-xs font-medium">按 DOI 补查元数据</p>
    <p class="text-[11px] leading-5 text-muted-foreground">
      优先使用数据库原始导出。没有可用导出时，可按名单 DOI 查询 Crossref
      登记记录，保留完整原响应；缺失内容继续核对。
    </p>
    <p class="break-all text-[11px]">
      名单 DOI：{{ task.record.doi || "未提供，请先核对原始来源" }}
    </p>
    <div class="flex flex-wrap gap-2">
      <Button
        variant="outline"
        size="sm"
        :disabled="locked || !task.record.doi.trim()"
        @click="lookup(false)"
        >{{ current ? "读取已保存来源" : "查询并保存来源" }}</Button
      >
      <Button
        v-if="current"
        variant="ghost"
        size="sm"
        :disabled="locked"
        @click="lookup(true)"
        >重新查询</Button
      >
    </div>
    <template v-if="current">
      <p class="break-all text-xs">
        {{ current.receipt.metadata.title.join(" / ") }}
      </p>
      <p class="break-all text-[11px] text-muted-foreground">
        {{ current.receipt.request.url }}
      </p>
      <p
        v-if="!current.receipt.matches_input_title"
        class="text-[11px] text-amber-700"
      >
        返回题名与名单不同，请核对 DOI 和完整题名；不会自动确认论文身份。
      </p>
      <p
        v-if="current.receipt.missing.length"
        class="text-[11px] text-amber-700"
      >
        来源未完整提供：{{ current.receipt.missing.join("、") }}。
      </p>
      <details class="text-[11px]">
        <summary class="cursor-pointer">完整元数据与原文件</summary>
        <p class="my-2 break-all text-muted-foreground">
          {{ current.receipt.archive_path }}
        </p>
        <p class="break-all font-mono text-muted-foreground">
          SHA-256：{{ current.receipt.sha256 }}
        </p>
        <pre
          class="mt-2 max-h-72 overflow-auto whitespace-pre-wrap break-all rounded-md bg-muted/40 p-2"
          >{{ JSON.stringify(current.receipt.metadata, null, 2) }}</pre>
      </details>
    </template>
  </div>
</template>
