<script setup lang="ts">
import type { Workspace } from "@/types";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  CardDescription,
} from "@/components/ui/card";
defineProps<{ batches: NonNullable<Workspace["legacy_materials"]> }>();
</script>
<template>
  <Card v-if="batches.length" class="shadow-none">
    <CardHeader
      ><CardTitle class="text-base">已迁入的历史材料</CardTitle
      ><CardDescription class="text-xs leading-6"
        >原文件和完整记录已保留。分类建议、准备结果和草稿仍需复核；未绑定资料不会用于当前任务。导出“任务与来源”可取得完整归档记录。</CardDescription
      ></CardHeader
    >
    <CardContent class="space-y-4 text-xs leading-6">
      <details
        v-for="(batch, index) in batches"
        :key="index"
        class="rounded-lg border p-3"
      >
        <summary class="cursor-pointer">
          批次 {{ index + 1 }} · {{ batch.records.length }} 条记录 ·
          {{ batch.files.length }} 个文件
        </summary>
        <p class="path mt-3">旧版目录：{{ batch.root }}</p>
        <div class="mt-3 max-h-64 space-y-3 overflow-auto">
          <div
            v-for="(record, i) in batch.records"
            :key="i"
            class="border-b pb-2"
          >
            <p>
              {{ record.title }} ·
              {{ record.kind === "classification" ? "分类建议" : "提交准备" }}
            </p>
            <p class="text-muted-foreground">
              {{
                record.task_ids.length
                  ? `对应 SA：${record.task_ids.join("、")}`
                  : "未绑定 · 名单版本或对应关系待核对"
              }}
            </p>
            <p
              v-for="(warning, j) in record.warnings"
              :key="j"
              class="break-all text-muted-foreground"
            >
              {{ warning }}
            </p>
          </div>
          <p
            v-for="(warning, i) in batch.warnings"
            :key="`warning-${i}`"
            class="break-all text-muted-foreground"
          >
            {{ warning }}
          </p>
        </div>
        <details class="mt-3">
          <summary class="cursor-pointer">查看原文件、保存位置和哈希</summary>
          <div class="mt-2 max-h-64 space-y-3 overflow-auto">
            <div
              v-for="file in batch.files"
              :key="file.original_file"
              class="border-b pb-2"
            >
              <p class="path">{{ file.original_file }}</p>
              <p class="path">{{ file.archive_path }}</p>
              <p class="path">SHA256：{{ file.sha256 }}</p>
            </div>
          </div>
        </details>
      </details>
    </CardContent>
  </Card>
</template>
