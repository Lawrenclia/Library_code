# Tauri 桌面工作台 · 0.2.2

当前 Windows 桌面应用使用 Tauri 2、Rust、SQLite、TypeScript 和 Vue 3。完整使用说明见 [主 README](../README.md)，业务规范见 [PPT 整体逻辑](../docs/TAURI_REBUILD_GOAL.md)。

## 编译

```powershell
pnpm install --frozen-lockfile
pnpm exec tauri build --bundles nsis -- --locked
```

默认输出 `src-tauri/target/release/bundle/nsis/`。GitHub 无需本地环境，按 [编译说明](../docs/GITHUB_ACTIONS_BUILD.md) 使用主分支工作流。生产构建自动编译 Vue/TypeScript 和 Rust，不运行测试或启动应用。

## 主要模块

- `src/`：工作台、来源入口、流程、材料与设置。
- `core/src/`：业务、SQLite、文件、模板、来源及各类队列。
- `src-tauri/src/commands/`：分类 IPC；验证本地主窗口。
- `src-tauri/src/engine/`：本库、SA、元数据、重复合并、认领、导入及写入恢复。
- `src-tauri/src/source_browser.rs`：独立渠道会话、原生下载、弹出窗口和租约交接。
- `core/src/source_flows.rs` 与 `src-tauri/src/commands/source_flows.rs`：渠道定位配置、完整范围核对、详情来源、原导出检查点和批量来源队列。
- `src-tauri/browser/`：按实际控件执行的页面驱动。
- `src-tauri/capabilities/`：本地与远程窗口能力范围。

运行时由一个 Engine 和 Store 管理状态；网页不能直接决定任务已完成。原意图、原始字节和来源版本冻结；不确定的副作用先回读，不能直接重发。

## 开发

`pnpm desktop` 启动开发应用；`pnpm build` 编译前端。本地测试及真实页面只读检查分别记录在 [0.2.1](../docs/TEST_RESULTS_0_2_1.md) 和 [0.2.2](../docs/TEST_RESULTS_0_2_2.md)。原生业务闭环测试使用隔离的本机模拟服务，未对真实平台写入；生产构建仍仅编译打包。

自动获取有两类驱动：WOS/CNKI 专用流程，以及其他渠道共用的、按真实控件和保存配置执行的来源流程。未提供各站的全部现场选择器或导出教程，不能把代码接入标记当作兼容性验收。完整实现范围见 [0.2.0 核对](../docs/TAURI_COMPLETION_AUDIT_0_2_0.md)。

组件来源与许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。旧版本操作记录保留在仓库 `docs/TAURI_TESTING_0_1_*.md`，以对应版本为准。
