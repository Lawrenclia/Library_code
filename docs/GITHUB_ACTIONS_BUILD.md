# 在 GitHub 编译 Windows EXE

无需在自己电脑安装 Rust、Node.js，也不需要填写 AI API token 或网站账号。

首轮已经编译并上传成功：打开 [Actions #1](https://github.com/Lawrenclia/Library_code/actions/runs/38063069025)，下载 `Windows-EXE-0.1.3-b6f8bb43`。压缩包内安装 EXE 为 6,445,536 字节，SHA256 为 `B0BDBDC4DA58C12980E2C087350DD5FAD1E3E88E6DB8358959619B81D01BD1FF`。这是首轮的固定源码版本；需要后续更新时按下方步骤重新编译。

1. 打开 [Windows EXE 工作流](https://github.com/Lawrenclia/Library_code/actions/workflows/windows-exe.yml)。
2. 点击 **Run workflow**，工作流分支保持 **main**。
3. `source_ref` 默认是 **feature/wos-download-workflow**，即当前 Tauri 新版源码；也可填包含 `desktop` 的其他分支、标签或提交 SHA。
4. 点击绿色 **Run workflow**，等待编译结束。
5. 打开成功的运行，在下方 **Artifacts** 下载 `Windows-EXE-版本-提交`，解压后运行其中的 `机构知识库工作台_版本_x64-setup.exe`。

产物包含安装包、`SHA256SUMS.txt` 和 `build-info.json`，记录实际编译的源码提交。产物保留 30 天，过期后可重新编译。首次运行需下载和编译 Rust 依赖；后续会复用缓存。

新版代码仍在 `feature/wos-download-workflow`，主分支仅登记构建入口，不把旧 Python 版本整体替换。该开发分支的 `desktop` 或工作流更新也会自动开始编译。将新版合入 main 后，可把 `source_ref` 改为 main。

流程只安装构建依赖、编译前端与 Rust、生成 NSIS 安装包并上传，不运行测试，不安装或启动生成的应用，不连接 WOS、CNKI 或机构后台，不上传业务数据。

编译失败时，展开标红的步骤查看原因；修复源码后重新运行。成功上传安装包才能说明云端打包完成，业务效果仍由用户自测。

配置依据：[Tauri 官方 GitHub 构建说明](https://v2.tauri.app/distribute/pipelines/github/)及 [GitHub 手动运行工作流说明](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow)。
