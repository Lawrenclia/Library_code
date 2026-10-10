<script setup lang="ts">
import { ref, watch } from "vue";
import type { Task, SubmissionOptions, SubmissionPrepared } from "@/types";
import type { DesktopCommand } from "@/services/desktop";
import { Button } from "@/components/ui/button";
import { Files, RefreshCw } from "@lucide/vue";
const props = defineProps<{
  task: Task;
  locked: boolean;
  run: <T>(
    command: DesktopCommand,
    args?: Record<string, unknown>,
    success?: string,
  ) => Promise<T | undefined>;
}>();
const options = ref<SubmissionOptions | null>(null);
const selected = ref("");
const prepared = ref<SubmissionPrepared | null>(null);
const stale = ref(false);
const names: Record<string, string> = {
  original: "原始数据库导出",
  field_valid: "模板字段校验通过",
  draft: "待完善草稿",
};
watch(
  () => props.task.id,
  () => {
    options.value = null;
    selected.value = "";
    prepared.value = null;
    stale.value = false;
  },
);
watch(
  () => props.task.revision,
  () => {
    if (options.value || prepared.value) stale.value = true;
  },
);
async function load() {
  const id = props.task.id;
  const result = await props.run<SubmissionOptions>(
    "submission_options",
    { id },
    "已读取本篇材料，请明确选择文件。",
  );
  if (
    !result ||
    props.task.id !== id ||
    result.task_revision !== props.task.revision
  )
    return;
  options.value = result;
  selected.value = "";
  prepared.value = result.prepared;
  stale.value = false;
}
async function prepare() {
  const scope = options.value,
    recipe = selected.value;
  if (!scope || !recipe || stale.value) return;
  const result = await props.run<SubmissionPrepared>(
    "prepare_submission",
    { id: scope.sa_id, recipe, taskRevision: scope.task_revision },
    "提交材料和来源版本已保存，请核对导入信息与待办事项。",
  );
  if (
    !result ||
    props.task.id !== scope.sa_id ||
    props.task.revision !== result.task_revision
  )
    return;
  prepared.value = result;
  stale.value = false;
  options.value = { ...scope, task_revision: result.task_revision };
}
</script>
<template>
  <section aria-label="提交材料准备" class="my-3 rounded-lg border p-3 text-xs">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <p class="flex items-center gap-2 font-medium">
        <Files class="size-4" />本篇提交材料
      </p>
      <Button size="sm" variant="outline" :disabled="locked" @click="load"
        ><RefreshCw class="size-3" />{{
          options ? "刷新可用材料" : "查看可用材料"
        }}</Button
      >
    </div>
    <p class="mt-2 leading-6 text-muted-foreground">
      明确选择本篇的原始导出或已生成模板，保存文件、渠道及导入说明。准备材料后仍需核验文献归属和本库缺失；原始导出优先。
    </p>
    <template v-if="options">
      <div v-if="options.choices.length" class="mt-3 space-y-2">
        <label
          v-for="choice in options.choices"
          :key="choice.recipe"
          class="flex gap-2 rounded-md bg-muted/40 p-2"
        >
          <input
            v-model="selected"
            type="radio"
            :value="choice.recipe"
            :disabled="locked || stale || !choice.usable"
            class="mt-1"
          />
          <span class="min-w-0 flex-1">
            <span class="block">{{ names[choice.kind] || choice.kind }}</span>
            <span class="mt-1 block break-all text-muted-foreground">{{
              choice.path
            }}</span>
            <span
              v-if="choice.issue"
              class="mt-1 block text-amber-700 dark:text-amber-400"
              >{{ choice.issue.message }}</span
            >
          </span>
        </label>
        <Button
          size="sm"
          :disabled="locked || stale || !selected"
          @click="prepare"
          >保存本篇提交材料</Button
        >
      </div>
      <p v-else class="mt-3 text-muted-foreground">
        此篇还没有可选择的材料。先获取原始导出，或按实际模板生成材料。
      </p>
    </template>
    <p v-if="stale" class="mt-2 text-amber-700 dark:text-amber-400">
      论文资料已更新，请刷新材料并重新核对。
    </p>
    <div v-if="prepared" class="mt-3 space-y-1 border-t pt-3 leading-6">
      <p>
        渠道：{{ prepared.packet.channel_label
        }}<span v-if="prepared.packet.work_type">
          · 类型建议：{{ prepared.packet.work_type }}</span
        >
      </p>
      <p>所属机构：{{ prepared.packet.organisation }}</p>
      <p>导入说明：{{ prepared.packet.instructions }}</p>
      <p class="break-all text-muted-foreground">
        {{ prepared.packet.material.path }}
      </p>
      <details>
        <summary class="cursor-pointer text-muted-foreground">
          文件与来源版本
        </summary>
        <p class="break-all">SHA256：{{ prepared.packet.material.sha256 }}</p>
        <p class="break-all">来源：{{ prepared.packet.material.audit }}</p>
      </details>
      <p v-if="prepared.can_upload && !stale" class="text-primary">
        已满足当前本地上传条件。下一步确认本条上传；程序会再次查询本库并回读
        SA。
      </p>
      <p
        v-for="issue in prepared.issues"
        :key="`${issue.code}-${issue.message}`"
        class="text-amber-700 dark:text-amber-400"
      >
        {{ issue.message }}
      </p>
    </div>
  </section>
</template>
