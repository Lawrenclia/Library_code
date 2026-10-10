<script setup lang="ts">
import { ref, watch } from "vue";
import type { Task } from "@/types";
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
const feedback = ref("");
watch(
  () => [props.task.id, props.task.sa_note?.plan_id],
  () => {
    feedback.value = "";
  },
);
async function execute(action: "prepare_note" | "verify_note") {
  feedback.value = "";
  await props.run(
    "run_step",
    { id: props.task.id, action, approved: false, extra: {} },
    action === "prepare_note"
      ? "独立备注已准备；请在平台保存备注后回读。"
      : "已回读到准确备注，SA 仍为待处理。",
  );
}
async function copyNote() {
  const note = props.task.sa_note?.note;
  if (!note) return;
  try {
    await navigator.clipboard.writeText(note);
    feedback.value = "已复制备注。请在平台单独保存备注，保持待处理状态。";
  } catch {
    feedback.value = "复制未成功，请选中下方备注手动复制。";
  }
}
</script>

<template>
  <div
    v-if="task.route === 'not_found'"
    class="my-3 rounded-lg border border-amber-200 bg-amber-50/50 p-4 dark:border-amber-900 dark:bg-amber-950/20"
  >
    <h3 class="text-sm font-medium">未查询到：仅保存备注</h3>
    <p class="my-2 text-xs leading-6 text-muted-foreground">
      先读取原 SA
      并准备备注，再在机构库可单独修改备注的入口保存，最后回读核验。此步骤由你在平台操作；“设置已处理”会改变状态，不能用于此分支。
    </p>
    <div class="flex flex-wrap gap-2">
      <Button
        variant="outline"
        size="sm"
        :disabled="locked || !task.review"
        @click="execute('prepare_note')"
        >1. 读取并准备备注</Button
      >
      <Button
        variant="outline"
        size="sm"
        :disabled="locked || !task.sa_note"
        @click="copyNote"
        >2. 复制备注</Button
      >
      <Button
        size="sm"
        :disabled="locked || !task.sa_note"
        @click="execute('verify_note')"
        >3. 回读备注与待处理状态</Button
      >
    </div>
    <template v-if="task.sa_note">
      <label class="mt-3 block text-xs">本次拟保存的完整备注</label>
      <textarea
        :value="task.sa_note.note"
        readonly
        aria-label="本次拟保存的完整备注"
        class="mt-2 min-h-20 w-full select-text rounded border bg-background p-3 text-xs leading-6"
      />
      <p
        v-if="task.sa_note.original_remark"
        class="mt-2 whitespace-pre-wrap text-xs leading-6 text-muted-foreground"
      >
        原备注：{{
          task.sa_note.original_remark
        }}。如需保留其中内容，先更新核验结论，再重新准备。
      </p>
      <p
        class="mt-2 text-xs leading-6"
        :class="
          task.sa_note.verified
            ? 'text-emerald-700 dark:text-emerald-300'
            : 'text-amber-800 dark:text-amber-300'
        "
      >
        {{
          task.sa_note.verified
            ? "最近回读已确认备注；SA 保持待处理，可继续后续查证。"
            : "交接记录已保存，尚未确认平台备注。任务保持待处理。"
        }}
      </p>
    </template>
    <p v-if="feedback" role="status" class="mt-2 text-xs leading-6">
      {{ feedback }}
    </p>
  </div>
</template>
