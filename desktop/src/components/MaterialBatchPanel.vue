<script setup lang="ts">
import { computed } from "vue";
import type { Workspace } from "@/types";
import type { DesktopCommand } from "@/services/desktop";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Files, Pause, FolderOpen } from "@lucide/vue";
const props = defineProps<{
  workspace: Workspace;
  owner: string;
  locked: boolean;
  run: <T>(
    command: DesktopCommand,
    args?: Record<string, unknown>,
    success?: string,
  ) => Promise<T | undefined>;
}>();
const batch = computed(() => props.workspace.material_batch);
const unfinished = computed(
  () => batch.value && !["completed", "cancelled"].includes(batch.value.status),
);
const states: Record<string, string> = {
  running: "正在整理",
  paused: "已暂停",
  interrupted: "重启后待继续",
  blocked: "存储受阻",
  completed: "本轮整理结束",
  cancelled: "原范围已结束",
};
const kinds = {
  original: "原始数据库导出",
  field_valid: "模板字段校验通过",
  draft: "模板草稿，仍有缺项或待核对要求",
};
const counts = computed(() => {
  const results = batch.value?.outcomes || [];
  return {
    original: results.filter((r) => r.product?.kind === "original").length,
    valid: results.filter((r) => r.product?.kind === "field_valid").length,
    draft: results.filter((r) => r.product?.kind === "draft").length,
    failed: results.filter((r) => r.error).length,
  };
});
</script>
<template>
  <section
    aria-label="批量材料整理"
    class="mb-4 rounded-xl border bg-card p-4 text-xs"
  >
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <p class="flex items-center gap-2 font-medium">
          <Files class="size-4 text-primary" />整理零匹配论文材料
        </p>
        <p class="mt-2 text-[11px] leading-6 text-muted-foreground">
          优先保留已核验的数据库原始导出；其他论文按已保存建议对应的实际模板生成材料和来源表。此步骤整理本地资料，不请求
          AI。缺少来源、建议或模板的论文会逐篇记录原因。
        </p>
      </div>
      <Button
        v-if="!unfinished"
        size="sm"
        :disabled="locked || !owner"
        @click="
          run(
            'prepare_material_batch',
            { owner },
            '材料与来源记录已保存，请核对逐篇结果。',
          )
        "
        >整理当前负责人的零匹配资料</Button
      >
    </div>
    <p v-if="!owner && !unfinished" class="mt-2 text-muted-foreground">
      先选择负责人。
    </p>
    <div v-if="batch" class="mt-4 border-t pt-3">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <p>
          {{ states[batch.status] }} · 原范围：{{ batch.owner }} · 已记录
          {{ batch.cursor }} / {{ batch.targets.length }} 篇
        </p>
        <div class="flex flex-wrap gap-2">
          <Button
            v-if="
              batch.status === 'running' &&
              workspace.running_service === 'materials'
            "
            size="sm"
            variant="outline"
            @click="
              run('pause_queue', {}, '已请求暂停，当前材料保存后停止后续整理。')
            "
            ><Pause />暂停后续整理</Button
          >
          <template v-else-if="unfinished">
            <Button
              size="sm"
              :disabled="locked"
              @click="
                run(
                  'resume_material_batch',
                  { id: batch.id },
                  '原范围的材料整理记录已保存。',
                )
              "
              >继续原材料范围</Button
            >
            <Button
              v-if="batch.status !== 'running'"
              size="sm"
              variant="outline"
              :disabled="locked"
              @click="
                run(
                  'cancel_material_batch',
                  { id: batch.id },
                  '已结束原范围，生成的材料和记录仍保留。',
                )
              "
              >结束原材料范围</Button
            >
          </template>
          <Button
            size="sm"
            variant="outline"
            :disabled="locked"
            @click="run('open_folder', { kind: 'materials' })"
            ><FolderOpen />打开材料目录</Button
          >
        </div>
      </div>
      <Progress
        class="mt-3 h-1.5"
        :model-value="
          batch.targets.length ? (batch.cursor / batch.targets.length) * 100 : 0
        "
      />
      <p class="mt-2 leading-6 text-muted-foreground">
        原始导出 {{ counts.original }} · 字段通过 {{ counts.valid }} ·
        待完善草稿 {{ counts.draft }} · 未生成 {{ counts.failed }}。文件位于
        materials/products，来源表位于
        materials/batches。继续保留原名单范围；字段校验通过仍需核对来源与平台要求。
      </p>
      <p
        v-if="batch.pause_requested && batch.status === 'running'"
        class="mt-2 text-muted-foreground"
      >
        已请求暂停，正在保存当前材料。
      </p>
      <details v-if="batch.outcomes.length" class="mt-2">
        <summary class="cursor-pointer">查看材料逐篇结果</summary>
        <ul class="mt-3 max-h-64 space-y-3 overflow-auto">
          <li v-for="result in batch.outcomes" :key="result.id">
            <p>
              SA {{ result.id }} ·
              {{ result.product ? kinds[result.product.kind] : "未生成"
              }}<span v-if="result.product?.reused"> · 复用原版本文件</span>
            </p>
            <p
              v-if="result.error"
              class="mt-1 text-amber-700 dark:text-amber-400"
            >
              {{ result.error.message }}
            </p>
            <template v-if="result.product">
              <p class="mt-1 break-all text-muted-foreground">
                {{ result.product.path }}
              </p>
              <p v-if="result.product.validation.missing.length" class="mt-1">
                必填缺项：{{ result.product.validation.missing.join("、") }}
              </p>
              <p
                v-for="problem in [
                  ...(result.product.validation.invalid || []),
                  ...(result.product.validation.requires_review || []),
                ]"
                :key="`${problem.column}-${problem.reason}`"
                class="mt-1 text-amber-700 dark:text-amber-400"
              >
                {{ problem.column }}：{{ problem.reason }}
              </p>
            </template>
          </li>
        </ul>
      </details>
    </div>
  </section>
</template>
