# 在 GitHub 编译 Windows EXE

当前应用为 Rust + TypeScript 的 Tauri 桌面版，版本以 `desktop/src-tauri/tauri.conf.json` 为准。编译无需 AI token 或机构库账号，也不需要本机安装 Rust、Node.js。

1. 打开 [Windows EXE 工作流](https://github.com/Lawrenclia/Library_code/actions/workflows/windows-exe.yml)。
2. 点击 **Run workflow**，工作流分支保持 **main**。
3. `source_ref` 默认 **main**；可填写包含 `desktop/` 的具体分支、标签或完整提交 SHA。需要重建同一版本时使用原提交 SHA。
4. 开始运行，等待“编译 Windows x64 安装包”成功。
5. 打开该次运行，在 **Artifacts** 下载 `Windows-EXE-版本-提交前8位`。
6. 解压后核对 `build-info.json` 和 `SHA256SUMS.txt`，再使用其中的 `机构知识库工作台_版本_x64-setup.exe`。

产物保留 30 天，过期可重新编译。首次运行需要下载 Rust 和前端依赖，后续会复用缓存。构建或观察超时不应连续启动多次同一任务；先查看已有运行状态。

## 自动编译范围

主分支的以下文件发生变化时自动构建：

- `desktop/**`
- `extension/adapter.js`、`extension/import-adapter.js`、`extension/wos-adapter.js`
- `.github/workflows/windows-exe.yml`

单独更新 README 或交付记录不会重新编译。工作流使用实际检出的源码 SHA，默认不依赖旧 Python 程序或 Chrome 扩展界面。

## 产物核对

`build-info.json` 记录应用版本、40 位源码提交、安装包文件名、实际字节数、SHA256、架构、运行链接和编译时间。`SHA256SUMS.txt` 可与实际 EXE 哈希比较。GitHub 包与本地编译包分别记录哈希，不要求两次构建字节相同。

本流程只安装依赖、编译前端和 Rust、生成 NSIS 安装包并上传。**不运行测试、不安装或启动应用、不连接业务网站、不上传业务数据。** 云端成功表示安装包生成，不能代替真实网站验收。

版本具体结果见 [交付记录](TAURI_ACCEPTANCE_STATUS.md)，0.2.0 使用步骤见 [版本说明](TAURI_TESTING_0_2_0.md)。
