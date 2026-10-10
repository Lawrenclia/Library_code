# 当前源码与安装包交付状态

记录日期：2026-10-10。代码检查范围为提交 `ddaa4a8`；本页仅整理源码入口与待验收事项，没有执行测试、构建、登录或平台写入。完整目标仍为 [重构规范](TAURI_REBUILD_GOAL.md)，历史证据见 [验收记录](TAURI_VERIFICATION.md)。源码存在不等于运行通过，历史测试也不证明后续修复已通过。

## 先确认正在运行哪个版本

现有 `D:\Library\releases\0.1.1\机构知识库工作台_0.1.1_x64-setup.exe` 的版本信息记录源提交为 `7a85082`，大小 6,239,660 字节，SHA256 为 `6B8FEAC0D8A2A6F066641825560053CB1EB21B7735CBA9C0024842FC6F9E0B90`。此处读取了版本信息，没有重新计算安装包哈希。

该安装包不包含 `7a85082` 之后的源码修改，包括后续来源候选定位、独立备注交接、新名单旧批次核验、迁移冲突预览和 Scopus CSV 来源登记。应用仍显示 0.1.1 时，不能仅凭这个版本号区分旧包和后续开发源码。源码尚未重新打包；更新 Git 仓库不会自动更新已安装程序。

## 按原验收条件逐项列出边界

下表编号对应重构规范第 7 节，不删减原验收范围。所有条目的当前源码运行结果均待验证；下列入口是后续核对位置，不是通过证明。

| 编号与要求 | 主要源码入口 | 仍需取得的验收证据 |
| --- | --- | --- |
| 1. PPT 各分支有完整步骤和完成条件 | `core/src/model.rs`、`workflow.rs`、`sa.rs`、`src-tauri/src/engine/` | 各分支实际输入、来源、执行和最终回读；未查询到分支目前是手动备注交接，独立编辑入口尚未确认 |
| 2. 零匹配四分支，仅缺失交大成果进入导入 | `core/src/library.rs`、`search_scopes.rs`、`model.rs` | 非交大、已有、缺失、未查询到四组结果及错误路径；查无勾选和网络失败不得替代真实查询 |
| 3. 单匹配按全部原因核对 | `core/src/issues.rs`、`metadata.rs`、`sa.rs` | 多原因任务逐项证据、角色和顺序核对、标识符对应同篇的原始来源 |
| 4. 重复合并及数字 2 跳过标记分离 | `core/src/files.rs`、`merge.rs`、`src-tauri/src/engine/duplicate_service.rs` | 实际主条目、合并前后完整字段、被合并项、SA 新匹配数；跳过任务不执行合并 |
| 5. 真实 WOS 完整记录 TXT 下载 | `extension/wos-adapter.js`、`core/src/download.rs`、`src-tauri/src/engine/downloads.rs` | 用户机构登录、核心合集、目标记录、原生完成事件、实际 TXT 字节与身份解析；截图或模拟不能代替 |
| 6. 原始导出优先、模板约束、AI 引用 | `core/src/catalog.rs`、`source_files.rs`、`materials.rs`、`templates.rs`、`src-tauri/src/ai.rs` | 实际导出和实际模板、完整必填及引用、缺项拒绝。其他数据库自动检索/导出/专用解析仍未实现 |
| 7. SA 说明、上传后导入、导入后五项推送 | `extension/import-adapter.js`、`core/src/model.rs`、`src-tauri/src/engine/writes.rs` | 真实文件、机构、批次、说明、计数、五项设置及各阶段回执。目前自动提交仅 WOS TXT |
| 8. 推送后查条目与 SA 核对/认领，未查询到保持未处理 | `core/src/sa.rs`、`claim.rs`、`alias.rs`、`src-tauri/src/engine/sa_service.rs` | 实际平台唯一号、工号/作者关系、必要别名、完整 SA 回读；下载/推送本身不能完成 SA |
| 9. 暂停/重启恢复、三次普通失败后继续、通道故障保留剩余 | `core/src/queue.rs`、`source_downloads.rs`、`ai_queue.rs`、`store.rs` | 当前源码的暂停和崩溃恢复结果、完整原队列、第四篇实际执行及通道故障未执行项 |
| 10. 未知写入先回读、不自动重复 | `core/src/store.rs`、`versions.rs`、`src-tauri/src/engine/writes.rs`、`submission_service.rs` | 每种写入在接收后中断的恢复和实际调用次数。上传原窗口丢失时已有批次回读路径；无可验证批次/服务器对象时仍保持未知，不可据此重传 |
| 11. 完整来源 Excel 字段和真实缺项 | `core/src/files.rs`、`submission_bundle.rs` | 当前源码实际生成报告、逐项原值/来源/哈希/阶段/批次和完整长文本对照，无记录处不得补造 |
| 12. 真实与本地证据分别标识 | 本页、`TAURI_VERIFICATION.md`、`tests/desktop-*-restart.test.cjs` | 本轮尚无测试或平台验收结果。必须保存当前源码的本地结果和真实运行证据后才能宣称整体验收完成 |

除 `extension` 外，上表代码入口均相对 `desktop`。Scopus CSV 是已登记的原始来源，不是已确认存在的机构库导入渠道；通用读取完整字段不等于数据库专用语义解析。

## 后续验证与交付

用户当前要求“先不验证”仍有效。本轮未调用测试、编译或真实页面操作。后续重新打包须在交付说明中更新源提交和实际文件信息；编译成功只能证明能生成程序，不能把上表业务验收改为通过。真实平台验收须在用户完成登录并授权相应操作后进行，不使用虚构记录或自动重发未知写入绕过。
