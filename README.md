# TD 工程启动器（TD Launcher）

给 TouchDesigner 用的工程启动器：一键新建工程文件夹，用缩略图浏览所有工程，按标签和状态分类。支持 Windows，macOS 版尚未打包测试。

## 功能

- **新建工程**：按模板建好文件夹（`assets/`、`tox/`、`scripts/`、`export/`）并用 TD 打开。
- **缩略图预览**：模板 `.toe` 里预埋了保存时自动存 `thumb.jpg` 的脚本。
- **标注与筛选**：标签、状态、备注写在每个工程文件夹的 `project.json` 里。
- **按文件夹分组**：卡片墙按你已有的分类文件夹自动分段显示。
- **导入工程**：把已有的 TD 工程文件夹登记进来，文件留在原处。
- **文件夹图标**（Windows）：工程文件夹在资源管理器里显示成带标志的文件夹。
- **自定义标志**：换成自己的标志，界面和文件夹图标一起换。
- **主题**：默认的黑白灰，和青玉色的深色主题「玉」。

## 目录

| 路径 | 内容 |
| --- | --- |
| `app/` | 软件源码（Tauri：界面用网页技术，系统操作用 Rust），用法和开发说明见 [`app/README.md`](app/README.md) |
| `logo/` | 玉字标志：彩色版、单色版（SVG）和 1024 像素的 PNG |
| `demo/` | 界面设计稿（单个 HTML 文件，浏览器直接打开） |
| `TD 工程启动器 · 项目企划.md` | 项目企划 |
| `TD 工程启动器 · 交接文档.md` | 交接文档 |

## 打包

需要 Node.js、Rust 和 Visual Studio C++ Build Tools。

```bash
cd app
npm install
npm run build
```

Windows 上也可以直接双击 `app/重新打包并安装.bat`。
