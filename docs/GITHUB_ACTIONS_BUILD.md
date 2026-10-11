# 在 GitHub 编译 Windows EXE

无需在自己电脑安装 Rust、Node.js，也不需要填写 AI API token 或网站账号。

主分支源码已更新为 **0.1.8**，新增来源格式配置及 RIS 题录读取；按下方工作流编译。0.1.8 的云端结果以对应运行记录为准，不用历史包证明新版成功。

已核对的 **0.1.7** 云端包：[Actions](https://github.com/Lawrenclia/Library_code/actions/runs/38101128343)，源码为 `769480200a635d328c5d819373c2b926ad94da91`，产物为 `Windows-EXE-0.1.7-76948020`。已下载 ZIP 并直接读取 EXE，实际大小 6,749,913 字节，SHA256 为 `877A5E9F89935EC282B66DADF8F7F07889D85020AB8CDE18D18E77475DA60F9B`，与包内版本、大小及哈希清单一致。该包包含 A 列空白筛选和 WOS 524 处理，不含 0.1.8 的来源更新；未安装或运行业务流程。云端和本地包分别记录哈希。

已核对的历史 **0.1.6** 云端包来自 [Actions](https://github.com/Lawrenclia/Library_code/actions/runs/38100737396)，产物为 `Windows-EXE-0.1.6-1c7d981b`，源码为 `1c7d981bcb17cb4c4210b22ffc204a059450187e`；实际安装 EXE 为 6,750,105 字节，SHA256 为 `56942E41CDA06D77F8F01F125F5AE7FDA23D6FC565E2F9D00A7D96E715156142`，与包内版本、大小和哈希清单一致。该包仅包含 A 列空白筛选，不包含 0.1.7 的 524 处理。云端和本地包分别记录，没有安装或运行程序。需要后续更新时按下方步骤重新编译。

1. 打开 [Windows EXE 工作流](https://github.com/Lawrenclia/Library_code/actions/workflows/windows-exe.yml)。
2. 点击 **Run workflow**，工作流分支保持 **main**。
3. `source_ref` 默认是 **main**，即当前 Tauri 新版源码；也可填包含 `desktop` 的其他分支、标签或提交 SHA。
4. 点击绿色 **Run workflow**，等待编译结束。
5. 打开成功的运行，在下方 **Artifacts** 下载 `Windows-EXE-版本-提交`，解压后运行其中的 `机构知识库工作台_版本_x64-setup.exe`。

产物包含安装包、`SHA256SUMS.txt` 和 `build-info.json`，记录实际编译的源码提交。产物保留 30 天，过期后可重新编译。首次运行需下载和编译 Rust 依赖；后续会复用缓存。

新版 Rust + TypeScript 桌面代码位于主分支的 `desktop`。主分支的 `desktop`、工作流，以及打入原生程序的 `extension/adapter.js`、`extension/import-adapter.js`、`extension/wos-adapter.js` 更新会自动开始编译；也可随时点击 **Run workflow** 手动编译。Python 和其余扩展界面保留旧版实现，新桌面 EXE 不依赖 Python。

流程只安装构建依赖、编译前端与 Rust、生成 NSIS 安装包并上传，不运行测试，不安装或启动生成的应用，不连接 WOS、CNKI 或机构后台，不上传业务数据。

编译失败时，展开标红的步骤查看原因；修复源码后重新运行。成功上传安装包才能说明云端打包完成，业务效果仍由用户自测。

配置依据：[Tauri 官方 GitHub 构建说明](https://v2.tauri.app/distribute/pipelines/github/)及 [GitHub 手动运行工作流说明](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow)。
