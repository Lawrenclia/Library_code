<script setup lang="ts">
import { ref, computed, watch } from "vue";
import type {
  Task,
  SourceBrowserState,
  SourceDownload,
  SourcePage,
} from "@/types";
import type { DesktopCommand } from "@/services/desktop";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
const props = defineProps<{
  task: Task;
  channel: string;
  locked: boolean;
  encoding: string;
  delimiter: string;
  run: <T>(
    command: DesktopCommand,
    args?: Record<string, unknown>,
    success?: string,
  ) => Promise<T | undefined>;
}>();
const emit = defineEmits<{ preview: [page: SourcePage, url: string] }>();
const state = ref<SourceBrowserState | null>(null),
  entry = ref(""),
  origins = ref("");
const downloads = computed(
  () =>
    state.value?.downloads.filter(
      (d) => d.session.site.channel === props.channel,
    ) || [],
);
const savedSite = computed(() =>
  state.value?.sites.find((s) => s.channel === props.channel),
);
const changedSite = computed(
  () =>
    entry.value.trim() !== (savedSite.value?.entry_url || "") ||
    origins.value.trim() !==
      (savedSite.value?.download_origins.join("\n") || ""),
);
const labels: Record<string, string> = {
  requested: "下载中",
  completed: "文件下载完成，待选择记录",
  failed: "下载失败",
  interrupted: "下载结束未确认",
};
watch(
  () => [props.task.id, props.channel],
  () => {
    state.value = null;
    entry.value = "";
    origins.value = "";
  },
);
async function load() {
  const id = props.task.id,
    channel = props.channel;
  const result = await props.run<SourceBrowserState>(
    "source_browser_state",
    { id },
    "已读取来源入口与原始下载记录。",
  );
  if (!result || props.task.id !== id || props.channel !== channel) return;
  state.value = result;
  const site = result.sites.find((s) => s.channel === channel);
  entry.value = site?.entry_url || "";
  origins.value = site?.download_origins.join("\n") || "";
}
async function save() {
  const id = props.task.id,
    channel = props.channel;
  const result = await props.run(
    "save_source_site",
    {
      site: {
        channel,
        entry_url: entry.value.trim(),
        download_origins: origins.value
          .split(/\r?\n/)
          .map((s) => s.trim())
          .filter(Boolean),
      },
    },
    "数据库入口已保存；网站检索和导出由你在内置窗口操作。",
  );
  if (result && props.task.id === id && props.channel === channel) await load();
}
async function open() {
  await props.run(
    "open_source_browser",
    { id: props.task.id, channel: props.channel },
    "来源窗口已打开。请自行完成登录、检索与原始导出；下载由应用保存。",
  );
}
async function preview(download: SourceDownload) {
  const id = props.task.id,
    channel = props.channel;
  const page = await props.run<SourcePage>(
    "preview_source_download",
    {
      id,
      downloadId: download.id,
      options: { encoding: props.encoding, delimiter: props.delimiter },
    },
    "原始下载已读取，请明确选择论文记录并填写绑定依据。",
  );
  if (
    page &&
    props.task.id === id &&
    props.channel === channel &&
    page.draft.task_revision === props.task.revision
  )
    emit("preview", page, download.page_url);
}
</script>
<template>
  <section class="space-y-2 rounded-lg border p-3" aria-label="内置来源浏览器">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <p class="text-xs font-medium">数据库原始下载</p>
      <Button size="sm" variant="outline" :disabled="locked" @click="load"
        >读取入口与下载记录</Button
      >
    </div>
    <p class="text-[11px] leading-6 text-muted-foreground">
      配置实际访问入口，在内置窗口自行登录、检索和导出。应用保存本篇的下载文件与来源，不把下载当作论文身份或交大归属确认。
    </p>
    <template v-if="state">
      <label class="field"
        >数据库入口<Input
          v-model="entry"
          :disabled="locked"
          placeholder="填写实际数据库网页地址"
      /></label>
      <details>
        <summary class="cursor-pointer text-[11px] text-muted-foreground">
          其他下载来源域名
        </summary>
        <Textarea
          v-model="origins"
          :disabled="locked"
          placeholder="每行一个实际下载来源，如 https://example.org；入口域名自动保留"
        />
        <p class="text-[10px] text-muted-foreground">
          只登记实际使用的页面、代理或导出文件来源。未登记的下载域名会被拒绝。
        </p>
      </details>
      <div class="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          :disabled="locked || !entry.trim()"
          @click="save"
          >保存入口</Button
        >
        <Button
          size="sm"
          :disabled="locked || !savedSite || changedSite"
          @click="open"
          >打开本篇来源窗口</Button
        >
      </div>
      <p v-if="changedSite" class="text-[11px] text-muted-foreground">
        先保存入口与下载来源，再打开窗口。
      </p>
      <div
        v-for="d in downloads"
        :key="d.id"
        class="space-y-1 rounded-md bg-muted/40 p-2 text-[11px]"
      >
        <p>{{ d.original_name }} · {{ labels[d.state] || d.state }}</p>
        <p class="break-all text-muted-foreground">{{ d.path }}</p>
        <p v-if="d.error" class="text-destructive">{{ d.error.message }}</p>
        <p
          v-if="d.session.input_hash !== task.input_hash"
          class="text-amber-700"
        >
          这是旧名单版本的下载。
        </p>
        <details>
          <summary class="cursor-pointer">来源与文件版本</summary>
          <p class="break-all">{{ d.page_url }}</p>
          <p class="break-all">SHA256：{{ d.sha256 || "未确认" }}</p>
        </details>
        <Button
          size="sm"
          variant="outline"
          :disabled="
            locked ||
            d.state !== 'completed' ||
            d.session.input_hash !== task.input_hash
          "
          @click="preview(d)"
          >选择文件中的论文记录</Button
        >
      </div>
      <p v-if="!downloads.length" class="text-[11px] text-muted-foreground">
        本渠道暂无下载记录。导出完成后刷新此列表，文件不会保存到系统 Downloads。
      </p>
    </template>
  </section>
</template>
