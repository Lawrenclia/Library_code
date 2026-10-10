<script setup lang="ts">
import { ref } from "vue";
import type { DesktopCommand } from "@/services/desktop";
import { Button } from "@/components/ui/button";
interface Settings {
  base: string;
  model: string;
  configured: boolean;
}
interface Plan extends Settings {
  path: string;
  fingerprint: string;
  bytes: number;
  supported: boolean;
  target_base: string;
}
const props = defineProps<{
  locked: boolean;
  configured: boolean;
  run: <T>(
    command: DesktopCommand,
    args?: Record<string, unknown>,
    success?: string,
  ) => Promise<T | undefined>;
}>();
const emit = defineEmits<{ imported: [settings: Settings] }>();
const plan = ref<Plan | null>(null);
async function preview() {
  const result = await props.run<Plan & { cancelled?: boolean }>(
    "preview_legacy_api_key",
    {},
    "已选择旧加密文件，请核对本机 API 地址和模型。",
  );
  if (result && !result.cancelled) plan.value = result;
}
async function importKey() {
  const selected = plan.value;
  if (!selected || props.configured || selected.configured) return;
  const result = await props.run<Settings>(
    "import_legacy_api_key",
    {
      path: selected.path,
      fingerprint: selected.fingerprint,
      base: selected.base,
      model: selected.model,
    },
    "旧版 API 密钥已存入系统凭据，旧文件和当前模型设置保持不变。",
  );
  if (result) {
    emit("imported", result);
    plan.value = null;
  }
}
</script>
<template>
  <section
    class="space-y-3 border-t pt-4 text-xs"
    aria-label="导入旧版 API 密钥"
  >
    <p class="font-medium">导入旧版 API 密钥</p>
    <p class="leading-6 text-muted-foreground">
      在原 Windows 账号下选择旧版 runtime 中的
      model_api_key.dpapi。密钥只保存在本机，不显示密钥、不调用
      API，当前模型设置保持不变。
    </p>
    <p v-if="configured" class="text-muted-foreground">
      当前密钥已配置。旧密钥导入不会覆盖现有设置。
    </p>
    <Button
      variant="outline"
      size="sm"
      :disabled="locked || configured"
      @click="preview"
      >选择旧加密文件</Button
    >
    <div v-if="plan" class="space-y-2 rounded-lg bg-muted/40 p-3 leading-6">
      <p class="break-all">{{ plan.path }}</p>
      <p class="break-all">当前 API：{{ plan.base }}</p>
      <p>当前模型：{{ plan.model }}</p>
      <p v-if="!plan.supported" class="text-amber-700">
        旧版加密文件只能在原 Windows 账号下导入。
      </p>
      <p
        v-else-if="plan.base.replace(/\/+$/, '') !== plan.target_base"
        class="text-amber-700"
      >
        旧密钥属于交大服务，请先保存交大 API 地址后重新选择文件。
      </p>
      <Button
        size="sm"
        :disabled="
          locked ||
          configured ||
          plan.configured ||
          !plan.supported ||
          plan.base.replace(/\/+$/, '') !== plan.target_base
        "
        @click="importKey"
        >将已选旧密钥导入本机</Button
      >
    </div>
  </section>
</template>
