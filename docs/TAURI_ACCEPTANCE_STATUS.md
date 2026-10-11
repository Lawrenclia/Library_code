# 当前源码与安装包交付状态

## 0.1.5 WOS 卡顿修复（2026-10-11）

工作台完整状态改为后台读取；任务、WOS 历史、未确认操作和名单版本在同一只读事务批量读取，前端合并刷新事件。WOS 使用新文档与唯一表单就绪探针、减少慢页面的扫描频率，按预期目标等待详情过渡，并显示各阶段等待秒数。检索暂停保留原队列位置；下载已经发出时仍等真实回执，不重复导出。详细限制见 [0.1.5 自测说明](TAURI_TESTING_0_1_5.md)。

前端生产构建、Rust release 与 NSIS 打包成功。安装包 `D:\Library\releases\0.1.5\机构知识库工作台_0.1.5_x64-setup.exe`，6,830,704 字节，SHA256 `FA6A6DB2084FFD220BDE5DDD1A1DC454AD1B0EC6C17DB2C39161B50B457C3EE0`。未运行测试或真实网站；尚无实际 WOS 计时、暂停恢复或业务通过结果，不将静态修复和编译成功写作现场验收完成。以下为旧版本交付记录。

主分支 **0.1.4 云端构建已成功**：[Actions](https://github.com/Lawrenclia/Library_code/actions/runs/38064502881) 全部步骤成功结束。源码为 `f19a13cf37e91623dd73cf04ad3b6ca9353fe507`，其文件树与合入的本地分支一致；产物 `Windows-EXE-0.1.4-f19a13cf` 已上传。已下载并直接读取 ZIP，安装 EXE 为 6,721,130 字节，实际 SHA256 `1B3EC70C70BC5123D342B33311DC2A7F5FDE6DC63A8B056F045F859394A1AB62` 与版本记录、哈希清单一致。云端 ZIP 保存在 `D:\Library\releases\github\0.1.4\f19a13cf`。云端与本地安装包分别记录哈希，未安装、启动或进行业务测试。

当前 **0.1.4** 增加 CNKI Excel 应用内另存：保留原件、完整单元格、另存文件和回执，并接入来源绑定、AI、材料准备及资料包。前端生产构建、Rust release 与 NSIS 打包成功，安装包 `D:\Library\releases\0.1.4\机构知识库工作台_0.1.4_x64-setup.exe` 为 6,793,753 字节，SHA256 `54ED9D4B775F5A2C1519D58B2D01F1F15ED2EFB965FD0A410EC0D212B580FEB3`。只编译打包，未运行测试或操作真实网站；CNKI 自动上传仍未实现。按用户要求，将本地新版合入 GitHub 主分支，Actions 默认源码及自动构建分支均为 `main`。步骤见 [0.1.4 自测说明](TAURI_TESTING_0_1_4.md)。下文保留较早版本的实际交付证据，不代表当前业务已经验收。

GitHub Windows EXE 首轮构建已成功结束：[Actions #1](https://github.com/Lawrenclia/Library_code/actions/runs/38063069025)。源码为 `b6f8bb437457d5cc7adfc692eeb130d7a4359330`，产物 `Windows-EXE-0.1.3-b6f8bb43` 包含安装 EXE、`build-info.json` 和 `SHA256SUMS.txt`。已下载并读取压缩包，安装 EXE 为 6,445,536 字节，实际 SHA256 与包内记录一致：`B0BDBDC4DA58C12980E2C087350DD5FAD1E3E88E6DB8358959619B81D01BD1FF`。该哈希属于 GitHub 构建，下面的本地构建是另一份文件，不应混用哈希。未安装或启动云端程序，业务测试与现场验收仍未执行。

此前主分支提交 `f97de7feb9ce822eba95fefe52b630897807a670` 只加入编译入口与说明，当时新版源码位于 `feature/wos-download-workflow`。0.1.4 后新版完整代码合入 `main`；可直接在 [Windows EXE](https://github.com/Lawrenclia/Library_code/actions/workflows/windows-exe.yml) 点击 **Run workflow**，默认编译主分支。下载和重编译步骤见 [GitHub 编译说明](GITHUB_ACTIONS_BUILD.md)。

新增 0.1.3：CNKI 内置浏览器题录获取的业务源码为 `ab0ad47`，自测步骤见 [0.1.3 说明](TAURI_TESTING_0_1_3.md)。支持官方详情页会话获取及内置网页原始下载，保留完整字段、原始响应和来源；平台 CNKI 自动上传/导入/推送仍未实现。以下 0.1.2 文件版本及原业务验收清单保留为此前记录；业务验证仍未执行。

0.1.3 前端生产构建、Rust release 编译和 NSIS 打包完成，未执行测试或真实网站操作。交付文件为 `D:\Library\releases\0.1.3\机构知识库工作台_0.1.3_x64-setup.exe`，6,525,659 字节，SHA256 `1C00F1B52F829F9505A8B451A613AFB4031A463EB74081C3FABD4F38C0B84B90`，实际构建源码 `ab0ad478d03ddccde755b590f7deb69f317e7dc0`。

记录日期：2026-10-10。业务源码入口依据 `ddaa4a8` 整理；最新 0.1.2 构建源码为 `1ba79ea`，增加版本及实际生成权限文件。用户已授权仅编译打包，前端生产构建、Rust release 编译与 NSIS 打包完成；没有执行测试、安装启动、登录或平台写入。完整目标仍为 [重构规范](TAURI_REBUILD_GOAL.md)，历史证据见 [验收记录](TAURI_VERIFICATION.md)。编译成功不等于业务运行通过，历史测试也不证明后续修复已通过。

## 先确认正在运行哪个版本

此前 0.1.2 包为 `D:\Library\releases\0.1.2\机构知识库工作台_0.1.2_x64-setup.exe`，源提交 `1ba79ea799eb8578e80df42a823a5792fa0bd71d`，大小 6,494,282 字节，实际 SHA256 为 `D6BF0FB146142ECC67A5836B61A97D6159C3EE9D8B0E57CA227EBA91EAEC6BC2`。未安装或启动；其自测顺序见 [0.1.2 说明](TAURI_TESTING_0_1_2.md)。0.1.3 新包应显示 **v0.1.3 测试版**。

旧 0.1.1 包仍固定对应 `7a85082`，不包含后续来源候选定位、独立备注交接、新名单旧批次核验、迁移冲突预览和 Scopus CSV 来源登记。0.1.2 已打包这些后续代码，旧安装包未覆盖。更新 Git 仓库不会自动更新已安装程序，仍显示 v0.1.1 时没有运行本次新版本。

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

用户当前要求“先不验证”仍有效；本轮仅按新授权编译打包 0.1.2，未运行测试或真实页面操作。编译成功只能证明能生成程序，不能把上表业务验收改为通过。真实平台验收须在用户完成登录并授权相应操作后进行，不使用虚构记录或自动重发未知写入绕过。
