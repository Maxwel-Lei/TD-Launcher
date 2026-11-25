# TD 工程启动器 · 软件

企划见上一级的《TD 工程启动器 · 项目企划.md》。界面来自 `../demo/index.html`。

## 直接用

安装包：`src-tauri/target/release/bundle/nsis/TD Launcher_0.2.0_x64-setup.exe`，双击安装（会覆盖旧版本，设置和标注都保留）。

改了代码以后重新打包：双击 `app` 文件夹里的 `重新打包并安装.bat`，它会打包并自动打开安装程序。

第一次打开会让你选**工程根目录**，比如 `F:\Projects\Touchdesigner Projects`。

- 只认两层文件夹：根目录直属的文件夹，里面直接有 `.toe`（或 `project.json`）的是一个工程，否则是一个分类文件夹（如 `01_商单`、`02_案例工程`）；分类文件夹直属的每个文件夹算一个工程。再往下的文件夹（组件、参考源文件、旧版本）都不单独算，工程的 `.toe` 放得深一点也能找到并打开。
- 跳过的文件夹：`Backup`、`_` 或 `.` 开头的、根目录第一层的 `00_Library`、Mac / Windows 系统杂项；崩溃存档不算主文件。
- 卡片墙默认按分类文件夹分组：根目录下第一层文件夹各占一段（设置里可以关）。侧栏"文件夹"按第一层分类筛选。
- 旧工程的预览图：没有 `thumb.jpg` 的工程，软件会在它的文件夹里自动找一张现成的图当预览（TD 导出的截图、导出文件夹里的图优先；只读，不往文件夹里写东西）。找得不对，在工程详情的预览图下面点"换预览图…"自己选，或"不用这张"。之后用 TD 保存出了 `thumb.jpg`，就自动换成它。
- 不能多开：软件已经开着时再点图标，不会开第二个窗口，而是把已有的窗口叫到前面。
- 没有 `project.json` 的老工程：点卡片墙上方的"识别并补标注"一次补上。没勾选的会被忽略，侧栏"全部恢复"可以找回。
- 导入根目录以外的工程：点右上角"导入"选文件夹，或把文件夹直接拖进窗口。选的文件夹本身是工程就直接导入；选的是上级文件夹，就列出里面所有工程让你勾选。导入是**原地登记**：文件不移动、不改名，只给没有标注的工程新增一个 `project.json`。导入的工程在卡片上标"外部"，并按它原来所在的文件夹分组（比如从 `02_案例工程` 里导入的就归在"02_案例工程"）。删除它们只是取消登记，不动文件；用批量操作"移动到…"可以把它们搬进根目录。
- 设置（左下角）：卡片墙按文件夹分组、给工程文件夹换图标、新建时加日期前缀、创建后自动用 TD 打开、切回时自动重新扫描。
- 主题（设置里）：自带「默认」（黑白灰）和「玉」（青玉色），每个主题都有浅色和深色两套，外观可以选浅色、深色或跟随系统。主题是文件：自带的在 `src/themes/`，自己做的放在"主题文件夹"里（设置里点"打开主题文件夹"），放进去点"重新读取"就能用，不用重新打包。格式和要求见 [`docs/theme-spec.md`](docs/theme-spec.md)。
- 自己做工程模板：新建工程时除了默认模板，还可以选自己做的。两种做法：在工程详情里点"存为模板"（存下文件夹结构和最新的 .toe），或者点"管理模板"打开模板文件夹，直接在里面建文件夹、放文件。每个文件夹就是一个模板，新建时整个复制成新工程。
- 自定义标志（设置里）：选一张图片（PNG 最好透明底、SVG、JPG），左上角的标志和工程文件夹图标都换成它；浅色标志会自动配深色底。存在软件的数据目录里，重装不丢，可以随时恢复默认。
- 文件夹图标（仅 Windows）：打开后，已标注的工程文件夹在资源管理器里显示成带标志的文件夹。做法是在工程文件夹里放两个隐藏文件 `desktop.ini` 和 `.td-folder.ico`；关掉设置就全部撤回。文件夹原来就有自己的 `desktop.ini` 的不会改。图标文件是 `src-tauri/resources/template/folder.ico`，想换图标就替换它再重新打包。
- 批量管理：Ctrl 点卡片多选、Shift 连选、Ctrl+A 全选，底部操作栏可改状态、加 / 去标签、移动到分类文件夹、移到回收站。

快捷键：`Ctrl+N` 新建工程，`F5` 重新扫描，`Ctrl+A` 全选，`Esc` 关闭弹窗或取消选择。从 TD 切回启动器时会自动重新扫描，刚存的缩略图会出现。

## 模板 .toe

新建工程时，软件会把 `src-tauri/resources/template/template.toe` 复制成 `工程名.toe`。模板里已经接好：

```
blank（黑色占位）→ out1 → thumb_res（640×360）→ thumb
thumb_saver（Execute DAT，Project Post Save 已打开）
```

用法：把你的最终画面接到 `out1`（或直接接 `thumb_res`），每次 `Ctrl+S` 都会在工程文件夹里存一张 `thumb.jpg`，切回启动器就能看到。TD 保存时会自动加版本号（`工程名.1.toe`、`.2.toe`…），启动器总是打开最新的那个。

想改模板：打开 TD，`Alt+T` 打开 Textport，粘贴回车（会把当前工程另存为模板，最好在空工程里做）：

```
exec(open(r'F:\Projects\VibeCoding\TDFileManager\app\tools\make_template.py', encoding='utf-8').read())
```

然后重新打包一次。如果删掉模板，新建工程仍会建好文件夹和 `project.json`，然后打开这个文件夹，让你在 TD 里新建并保存进去。

## 开发

需要 Node.js、Rust、Visual Studio C++ Build Tools（这台电脑已装好）。

```bash
npm run dev      # 开发模式，改了前端代码后关掉窗口重新运行
npm run build    # 打包安装程序
```

```bash
cd src-tauri && cargo test   # 后端测试
```

不开软件也能看界面：在 `app` 目录运行 `python -m http.server 8765`，浏览器打开 `http://localhost:8765/tools/preview.html`（用假数据）。

## 文件

| 路径 | 内容 |
| --- | --- |
| `src/index.html` | 界面和样式 |
| `src/app.js` | 界面逻辑、自定义标志的合成 |
| `src-tauri/src/lib.rs` | 后端：扫描、新建、导入、读写 project.json、打开、在文件夹中显示、移动、文件夹图标 |
| `src-tauri/resources/template/folder.ico` | 工程文件夹图标 |
| `重新打包并安装.bat` | 一键打包并打开安装程序 |
| `src-tauri/tauri.conf.json` | 窗口、打包设置 |
| `src/themes/` | 自带的主题（每个主题一个 JSON 文件，`index.json` 里登记）和主题的标志 |
| `docs/theme-spec.md`、`docs/theme-template.json` | 主题制作规范和模板，打包时会带进软件 |
| `tools/make_template.py` | 在 TD 里生成模板 .toe |
| `tools/preview.html`、`tools/mock-tauri.js` | 浏览器预览用的假后端 |
