# 当前源码与安装包交付状态

## 0.1.8 来源格式与 RIS 题录（2026-10-11）

源码新增独立来源格式配置及 RIS 完整记录解析，接入来源文件选择、原生下载接收、记录定位、人工绑定、AI 来源核验和来源报告；常见表格身份列可预填，仍需核对具体记录。全部原始字节、字段与完整记录保留。RIS 只作为事实来源供 AI/模板准备，不扩展机构库上传格式，不进入原始上传材料配方；非 WOS TXT 自动提交仍未实现。其他数据库专用自动检索和导出也没有因通用 RIS 读取而完成。操作说明见 [0.1.8](TAURI_TESTING_0_1_8.md)。新增解析和接入回归用例源码，按用户要求未运行测试或真实网站操作，整体验收尚未通过。

0.1.8 最终源码 `112a539f32925aba3bc22a3c73cc3910c7249cd9` 已推送主分支；前端生产构建、Rust release 编译和 NSIS 打包成功。本地安装包 `D:\Library\releases\0.1.8\机构知识库工作台_0.1.8_x64-setup.exe` 为 6,854,956 字节，实际 SHA256 为 `943F493354E7C80D166FB8D252AF81B0731A03806EEB12DA45CBDA8C5D11E377`，已与版本信息核对一致，同目录保存自测说明。未安装启动、未执行测试或真实网站操作。

[对应 GitHub Actions](https://github.com/Lawrenclia/Library_code/actions/runs/38102276931) 已成功完成，产物 `Windows-EXE-0.1.8-112a539f`（Artifact ID `11688062839`）已上传。已下载 ZIP 并直接读取实际 EXE：6,747,538 字节，SHA256 为 `DDE1ACAF47283C99E192D9FEE15482D5D8ACEBA283316FD0AF568871D4E3482A`，与包内版本、源码、大小和 SHA256 清单一致。ZIP 位于 `D:\Library\releases\github\0.1.8\112a539f`。云端与本地包分别记录哈希；没有安装启动或业务验收，云端编译成功不证明平台流程可用。

## 0.1.7 WOS 站点故障处理（2026-10-11）

GitHub [本次 Actions](https://github.com/Lawrenclia/Library_code/actions/runs/38101128343) 已成功完成，源码 `769480200a635d328c5d819373c2b926ad94da91`，产物 `Windows-EXE-0.1.7-76948020`。已下载 ZIP 并读取实际 EXE：6,749,913 字节，SHA256 为 `877A5E9F89935EC282B66DADF8F7F07889D85020AB8CDE18D18E77475DA60F9B`，与包内版本、源码、大小和哈希清单一致。ZIP 位于 `D:\Library\releases\github\0.1.7\76948020`。没有安装或启动；云端包与下面的本地包分别记录哈希。

新增 Cloudflare 524 等明确 5xx 错误页识别；WOS 页面准备、表单准备、结果等待和导出准备的最终超时暂停原队列，保留当前位置与剩余论文。下载结果未知保留原生回执并阻止队列推进，不重新导出。短暂页面探针超时仍在原期限内观察，明确数据库零结果仍继续下一篇，没有恢复连续三条失败终止规则。保留 0.1.6 的物理 A 列空白筛选和重新导入范围保护。GitHub EXE 自动构建也追踪三个打入原生程序的共享页面适配器，避免仅修改适配器后没有生成新版 EXE。

前端生产构建、Rust release 编译和 NSIS 打包成功。本地安装包 `D:\Library\releases\0.1.7\机构知识库工作台_0.1.7_x64-setup.exe` 为 6,843,471 字节，SHA256 为 `C9B9B33A6476C545283154837C3818C96BF135CC7999F888E7D717678A1CA204`。源码提交记录在同目录 `版本信息.json`。新增队列重启与剩余任务保护的回归用例源码，但按用户要求未运行测试；未安装启动或操作真实网站。完整目标仍未现场验收，编译不能证明实际暂停恢复、防重复提交和平台业务闭环。使用说明见 [0.1.7 自测步骤](TAURI_TESTING_0_1_7.md)。

此前 0.1.6 的 A 列筛选源码为 `1c7d981bcb17cb4c4210b22ffc204a059450187e`，本地安装包 `D:\Library\releases\0.1.6\机构知识库工作台_0.1.6_x64-setup.exe` 为 6,828,756 字节，SHA256 为 `3130FC3F8782072CE0E3FD1D6CBDFAC4E886EFA88F472638A9E55F33C4CDD7A2`。该包未包含本节 WOS 站点故障处理。下文保留历史证据。

## 0.1.5 WOS 卡顿修复（2026-10-11）

GitHub [本次 Actions](https://github.com/Lawrenclia/Library_code/actions/runs/38099085080) 全部步骤成功结束，源码 `0a4bda7ec1fd67aad20335736d54798eea9d9468`，产物 `Windows-EXE-0.1.5-0a4bda7e` 已上传。直接读取下载 ZIP，确认版本、源码提交、文件名、实际 EXE 大小 6,741,127 字节和 SHA256 `EB5817C0F6C10F4092A2AA95036FD040CDDE5CBC7381F036C7B0FB49DC9453E7` 与包内记录一致；ZIP 保存在 `D:\Library\releases\github\0.1.5\0a4bda7e`。云端构建与本地构建分别记录，不安装启动、不运行业务测试。

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
| 4. 重复合并及 A 列空白输入范围分离 | `core/src/files.rs`、`store.rs`、`merge.rs`、`src-tauri/src/engine/duplicate_service.rs` | 实际主条目、合并前后完整字段、被合并项、SA 新匹配数；A 列任意非空行不加入范围，旧任务及旧队列不得绕过新范围 |
| 5. 真实 WOS 完整记录 TXT 下载 | `extension/wos-adapter.js`、`core/src/download.rs`、`src-tauri/src/engine/downloads.rs` | 用户机构登录、核心合集、目标记录、原生完成事件、实际 TXT 字节与身份解析；截图或模拟不能代替 |
| 6. 原始导出优先、模板约束、AI 引用 | `core/src/catalog.rs`、`source_files.rs`、`ris.rs`、`cnki.rs`、`cnki_excel.rs`、`materials.rs`、`templates.rs`、`src-tauri/src/ai.rs` | 实际导出和实际模板、完整必填及引用、缺项拒绝。CNKI 已有导出响应、专用文本解析及 Excel 另存源码；0.1.8 已有通用 RIS 读取，供来源核对及模板准备。这些均未现场验收；其他数据库自动检索/导出/专用解析仍未实现 |
| 7. SA 说明、上传后导入、导入后五项推送 | `extension/import-adapter.js`、`core/src/model.rs`、`src-tauri/src/engine/writes.rs` | 真实文件、机构、批次、说明、计数、五项设置及各阶段回执。目前自动提交仅 WOS TXT |
| 8. 推送后查条目与 SA 核对/认领，未查询到保持未处理 | `core/src/sa.rs`、`claim.rs`、`alias.rs`、`src-tauri/src/engine/sa_service.rs` | 实际平台唯一号、工号/作者关系、必要别名、完整 SA 回读；下载/推送本身不能完成 SA |
| 9. 暂停/重启恢复、三次普通失败后继续、通道故障保留剩余 | `core/src/queue.rs`、`source_downloads.rs`、`ai_queue.rs`、`store.rs`、`src-tauri/src/browser.rs` | 当前源码的暂停和崩溃恢复结果、完整原队列、第四篇实际执行及 524/最终超时后的当前目标与未执行范围；现有回归用例未运行 |
| 10. 未知写入先回读、不自动重复 | `core/src/store.rs`、`versions.rs`、`src-tauri/src/engine/writes.rs`、`submission_service.rs` | 每种写入在接收后中断的恢复和实际调用次数。上传原窗口丢失时已有批次回读路径；无可验证批次/服务器对象时仍保持未知，不可据此重传 |
| 11. 完整来源 Excel 字段和真实缺项 | `core/src/files.rs`、`submission_bundle.rs` | 当前源码实际生成报告、逐项原值/来源/哈希/阶段/批次和完整长文本对照，无记录处不得补造 |
| 12. 真实与本地证据分别标识 | 本页、`TAURI_VERIFICATION.md`、`tests/desktop-*-restart.test.cjs` | 本轮尚无测试或平台验收结果。必须保存当前源码的本地结果和真实运行证据后才能宣称整体验收完成 |

除 `extension` 外，上表代码入口均相对 `desktop`。Scopus CSV 是已登记的原始来源，不是已确认存在的机构库导入渠道；通用读取完整字段不等于数据库专用语义解析。

## 后续验证与交付

用户当前要求“先不验证”仍有效；最近交付按授权编译打包 0.1.8，本地与 GitHub 安装包均已核对，未运行测试或真实页面操作。编译成功只能证明能生成程序，不能把上表业务验收改为通过。非 WOS 渠道的实际导入窗口、回执和记录标识，以及独立备注保存入口，仍缺页面合同依据；原 PPT 与现有模拟夹具只有 WOS TXT 提交流程，不能证明其他渠道共用其组件和字段。补齐这些驱动需要实际页面资料；完整验收还需要当前源码测试及已登录平台的真实运行证据。真实平台验收须在用户完成登录并授权相应操作后进行，不使用虚构记录或自动重发未知写入绕过。
