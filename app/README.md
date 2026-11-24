# TD 工程启动器 · 软件

企划见上一级的《TD 工程启动器 · 项目企划.md》。界面来自 `../demo/index.html`。

## 直接用

安装包：`src-tauri/target/release/bundle/nsis/TD Launcher_0.2.0_x64-setup.exe`，双击安装（会覆盖旧版本，设置和标注都保留）。

改了代码以后重新打包：双击 `app` 文件夹里的 `重新打包并安装.bat`，它会打包并自动打开安装程序。

第一次打开会让你选**工程根目录**，比如 `F:\Projects\Touchdesigner Projects`。

- 根目录里的分类文件夹（如 `01_商单`、`02_案例工程`）会自动往里找，最多 4 层。
- 识别规则见企划 §6.1：跳过 `Backup`、`_` 开头的文件夹、`00_Library`、Mac / Windows 系统杂项、崩溃存档；工程里附带的 `.toe` 不算单独的工程。
- 侧栏"文件夹"按第一层分类筛选。
- 没有 `project.json` 的老工程：点卡片墙上方的"识别并补标注"一次补上。没勾选的会被忽略，侧栏"全部恢复"可以找回。
- 导入根目录以外的工程：点右上角"导入"选文件夹，或把文件夹直接拖进窗口。选的文件夹本身是工程就直接导入；选的是上级文件夹，就列出里面所有工程让你勾选。导入是**原地登记**：文件不移动、不改名，只给没有标注的工程新增一个 `project.json`。导入的工程在卡片上标"外部"，侧栏"文件夹"里归在"外部导入"。删除它们只是取消登记，不动文件；用批量操作"移动到…"可以把它们搬进根目录。
- 设置（左下角）：给工程文件夹换图标、新建时加日期前缀、创建后自动用 TD 打开、切回时自动重新扫描。
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
| `src/app.js` | 界面逻辑、渐变像素装饰、自定义标志的合成 |
| `src-tauri/src/lib.rs` | 后端：扫描、新建、导入、读写 project.json、打开、在文件夹中显示、移动、文件夹图标 |
| `src-tauri/resources/template/folder.ico` | 工程文件夹图标 |
| `重新打包并安装.bat` | 一键打包并打开安装程序 |
| `src-tauri/tauri.conf.json` | 窗口、打包设置 |
| `tools/make_template.py` | 在 TD 里生成模板 .toe |
| `tools/preview.html`、`tools/mock-tauri.js` | 浏览器预览用的假后端 |
