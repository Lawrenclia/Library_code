<script setup lang="ts">
import { ref, watch, onMounted } from "vue";
import {
  BookOpen,
  LayoutDashboard,
  Files,
  Settings2,
  FolderOpen,
  ShieldCheck,
  Moon,
  Sun,
  LoaderCircle,
  Workflow,
} from "@lucide/vue";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarRail,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
const props = defineProps<{
  page: string;
  count: number;
  running: boolean;
  configured: boolean;
}>();
const emit = defineEmits<{ "update:page": [value: string]; folder: [] }>();
const entries = [
  { id: "workflow", label: "流程与连接", icon: Workflow },
  { id: "tasks", label: "任务工作台", icon: LayoutDashboard },
  { id: "materials", label: "模板与材料", icon: Files },
  { id: "settings", label: "模型与设置", icon: Settings2 },
];
const dark = ref(localStorage.getItem("workspace-theme") === "dark");
watch(dark, (value) => {
  document.documentElement.classList.toggle("dark", value);
  localStorage.setItem("workspace-theme", value ? "dark" : "light");
});
onMounted(() => document.documentElement.classList.toggle("dark", dark.value));
</script>
<template>
  <SidebarProvider
    :default-open="true"
    style="--sidebar-width: 14rem; --sidebar-width-icon: 4rem"
  >
    <Sidebar collapsible="icon" variant="inset" class="border-none">
      <SidebarHeader class="gap-0 p-3"
        ><SidebarMenu
          ><SidebarMenuItem
            ><SidebarMenuButton
              size="lg"
              class="gap-3 hover:bg-transparent"
              @click="emit('update:page', 'tasks')"
              ><div
                class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-primary text-primary-foreground"
              >
                <BookOpen class="size-5" />
              </div>
              <div class="grid flex-1 text-left leading-tight">
                <span class="text-sm font-semibold">机构知识库</span
                ><span class="mt-1 text-xs text-muted-foreground"
                  >科研成果工作台</span
                >
              </div></SidebarMenuButton
            ></SidebarMenuItem
          ></SidebarMenu
        ></SidebarHeader
      >
      <SidebarContent
        ><SidebarGroup
          ><SidebarGroupLabel class="mb-2 text-[10px] tracking-wider"
            >工作空间</SidebarGroupLabel
          ><SidebarGroupContent
            ><SidebarMenu class="gap-1.5"
              ><SidebarMenuItem v-for="entry in entries" :key="entry.id"
                ><SidebarMenuButton
                  :is-active="page === entry.id"
                  :tooltip="entry.label"
                  :aria-label="entry.label"
                  @click="emit('update:page', entry.id)"
                  class="h-10 gap-3 rounded-lg px-3"
                  ><component :is="entry.icon" /><span>{{ entry.label }}</span
                  ><span
                    v-if="entry.id === 'tasks'"
                    class="ml-auto rounded bg-background/70 px-1.5 text-[10px] tabular-nums group-data-[collapsible=icon]:hidden"
                    >{{ count }}</span
                  ></SidebarMenuButton
                ></SidebarMenuItem
              ></SidebarMenu
            ></SidebarGroupContent
          ></SidebarGroup
        ></SidebarContent
      >
      <SidebarFooter class="gap-3 p-3"
        ><div
          class="rounded-xl border bg-background/70 p-3 group-data-[collapsible=icon]:hidden"
        >
          <ShieldCheck class="mb-2 size-4 text-primary" />
          <p class="text-xs font-medium">每一步，都有依据</p>
          <p class="mt-1 text-[11px] leading-relaxed text-muted-foreground">
            按照 PPT 核验文献，来源与进度自动保存。
          </p>
        </div>
        <SidebarMenu
          ><SidebarMenuItem
            ><SidebarMenuButton tooltip="打开工作目录" @click="emit('folder')"
              ><FolderOpen /><span>打开工作目录</span></SidebarMenuButton
            ></SidebarMenuItem
          ></SidebarMenu
        >
        <div
          class="flex justify-between px-2 text-[10px] text-muted-foreground group-data-[collapsible=icon]:hidden"
        >
          <span>本地自动保存</span><span>v0.1.3 测试版</span>
        </div></SidebarFooter
      ><SidebarRail />
    </Sidebar>
    <SidebarInset class="min-w-0 overflow-hidden bg-background shadow-none!">
      <header class="flex h-14 shrink-0 items-center gap-3 border-b px-6">
        <SidebarTrigger aria-label="展开或收起侧栏" /><Separator
          orientation="vertical"
          class="h-4!"
        /><span class="text-xs text-muted-foreground">工作空间</span
        ><span class="text-xs text-muted-foreground/50">/</span
        ><span class="text-xs font-medium">{{
          entries.find((e) => e.id === page)?.label
        }}</span>
        <div class="ml-auto flex items-center gap-3">
          <span
            v-if="running"
            class="flex items-center gap-1.5 text-xs text-primary"
            ><LoaderCircle class="size-3 animate-spin" />任务进行中</span
          ><span
            v-else
            class="hidden items-center gap-1.5 text-[11px] text-muted-foreground md:flex"
            ><span
              class="size-1.5 rounded-full"
              :class="configured ? 'bg-emerald-500' : 'bg-muted-foreground/50'"
            ></span
            >{{ configured ? "API 已配置" : "API 未配置" }}</span
          ><Button
            size="icon-sm"
            variant="ghost"
            :aria-label="dark ? '切换浅色主题' : '切换深色主题'"
            @click="dark = !dark"
            ><Sun v-if="dark" /><Moon v-else
          /></Button>
        </div>
      </header>
      <div class="min-w-0 p-5 xl:p-7"><slot /></div>
    </SidebarInset>
  </SidebarProvider>
</template>
