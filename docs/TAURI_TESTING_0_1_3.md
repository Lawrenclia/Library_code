# 0.1.3 自测说明

本版增加 CNKI 内置浏览器获取题录，包含 0.1.2 的已有功能。只编译打包，未运行测试或操作真实网站。

也可从 [GitHub Actions 首轮构建](https://github.com/Lawrenclia/Library_code/actions/runs/38063069025) 的 **Artifacts** 下载 `Windows-EXE-0.1.3-b6f8bb43`，解压后安装其中的 EXE。后续源码可通过 [Windows EXE](https://github.com/Lawrenclia/Library_code/actions/workflows/windows-exe.yml) 手动编译，步骤见 [GitHub 编译说明](GITHUB_ACTIONS_BUILD.md)。不需要使用开发者电脑上的 `D:\Library` 路径。

1. 自行退出旧工作台，安装 `D:\Library\releases\0.1.3\机构知识库工作台_0.1.3_x64-setup.exe`；界面应显示 **v0.1.3 测试版**。程序不会自动安装或关闭旧窗口。
2. 选择需要补充来源的论文，在「来源」选择 CNKI，点击「按名单题名打开 CNKI」。首次使用不用填写官方入口。
3. 在内置窗口完成登录、机构访问或验证码。按实际题名、作者和年份核对结果，打开单篇论文详情。
4. 回到工作台，在该窗口卡片点击「获取当前详情页题录」。获取成功后显示 TXT 和原始 JSON 的保存位置；题名冲突、多篇、登录验证或不支持的页面会给出错误。
5. 在预览中明确选择论文行，核对题名/DOI 列、作者、单位、年份、摘要以及缺项，填写绑定依据并确认。绑定后可调用已配置 API AI 分类、填写已登记模板；AI 缺少依据的内容仍须补充来源。
6. 需要数据库原始上传文件时，在内置网页手动导出 Excel/TXT。工作台接收下载到工作目录 `downloads/sources`；完成后在列表选择该文件并绑定记录。CNKI 会话获取的整理 TXT 不作为原始上传文件候选。
7. 若导出的 XLS 实际为网页表格，使用 Excel 打开并另存为 XLSX，再在来源中选择新文件；工作台保留原下载，不猜测其表格结构。

CNKI 自动获取只接通官方 HTTPS 单篇详情页公开的 GetExport。机构代理可保存实际入口，在内置网页操作并导出。CNKI 自动上传、导入、推送尚未接通；其他渠道仍可配置内置来源窗口并接收原始下载。

实现与保存约定见 [CNKI 内置浏览器说明](CNKI_BROWSER_DOWNLOAD.md)。若失败，保留具体错误、内置窗口地址和实际导出文件格式，便于继续修复。
