# 前端开源来源

工作台采用 [shadcn-vue](https://github.com/unovue/shadcn-vue) 的官方 `new-york-v4` 开源组件，按照官方 [Dashboard / Sidebar](https://www.shadcn-vue.com/blocks) 结构适配科研任务。实际使用的组件源码位于 `src/components/ui`，来自官方注册表，依赖 Reka UI、VueUse、Tailwind CSS 与 Lucide。

shadcn-vue 的 MIT 原文保留在 `licenses/shadcn-vue.LICENSE`。每个下载组件的官方地址与完整响应 SHA-256 保存在 `licenses/shadcn-registry.json`。应用没有引用演示站的用户资料、统计数据或假后端。

Windows 安装程序同时附带 MIT 许可与本说明，位于应用安装目录。

`WorkbenchShell.vue`、`TaskTable.vue` 和业务页面为本项目的适配代码。`maintenance/vendor_shadcn.py` 可以重新获取官方组件；更新后必须重新检查 UI 与构建。
