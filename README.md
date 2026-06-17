# ClipForge

## 中文

ClipForge 是一个基于 Tauri、React 和 Rust 构建的桌面剪贴板历史工具。它常驻系统托盘，在后台自动记录文本和图片剪贴板内容，并通过全局快捷键快速呼出一个轻量窗口，方便搜索、分类、复制并粘贴历史内容。

### 项目用途

日常开发、写作和资料整理时，经常需要在多个应用之间重复复制代码片段、链接、邮箱、颜色值、图片或普通文本。ClipForge 的目标是把这些临时剪贴板内容沉淀为可检索、可分组、可快速粘贴的历史记录，减少反复查找和重复复制的成本。

### 核心功能

- 剪贴板历史：后台监听系统剪贴板，保存文本和图片记录。
- 快速呼出：默认使用 `CmdOrCtrl+Shift+V` 显示或隐藏主窗口。
- 智能分类：自动识别链接、邮箱、代码、颜色、图片和普通文本。
- 搜索与分页：支持关键词搜索和无限滚动加载历史记录。
- 一键粘贴：点击历史项后复制到系统剪贴板，并模拟粘贴到之前的应用。
- 图片支持：保存图片剪贴板内容，并可从历史记录重新粘贴图片。
- 自定义分组：创建、重命名、删除分组，为分组设置颜色，并将记录加入分组。
- 历史清理策略：可设置最多保留条数和最长保存天数，已加入分组的记录会被保护。
- 系统托盘：关闭窗口时隐藏到托盘，托盘菜单支持显示窗口和退出应用。
- 启动配置：支持开机自启动、快捷键配置、数据库路径配置。

### 技术架构

```text
ClipForge
├─ Frontend: React + TypeScript + Vite
│  ├─ src/App.tsx        主界面、设置页、搜索、分组、历史列表
│  ├─ src/App.css        桌面窗口样式
│  └─ src/shortcuts.ts   默认快捷键常量
├─ Desktop Runtime: Tauri 2
│  ├─ src-tauri/src/lib.rs       应用入口、插件注册、命令注册
│  ├─ src-tauri/src/clipboard.rs 剪贴板监听、复制粘贴、图片读取
│  ├─ src-tauri/src/db.rs        SQLite 存储、迁移、分类、分组、清理策略
│  ├─ src-tauri/src/shortcut.rs  全局快捷键注册和窗口定位
│  ├─ src-tauri/src/tray.rs      系统托盘和窗口隐藏逻辑
│  ├─ src-tauri/src/config.rs    外部 JSON 配置
│  └─ src-tauri/src/autostart.rs 开机自启动
└─ Storage
   ├─ SQLite: clipboard_items、custom_groups、config
   └─ Images: 图片剪贴板内容保存为 PNG 文件
```

前端负责窗口交互、筛选、设置和展示；Rust 后端负责系统能力，包括剪贴板访问、全局快捷键、托盘、数据库、图片文件管理和模拟粘贴。前后端通过 Tauri commands 通信。

### 数据存储

默认数据库路径为应用可执行文件同级的 `db/clipforge.db`，图片保存到同一目录下的 `images/`。也可以在设置页选择自定义数据库路径。外部配置会写入可执行文件同级的 `clipforge.json`。

### 开发环境

需要安装：

- Node.js
- Rust stable toolchain
- 系统所需的 Tauri 依赖

安装依赖：

```bash
npm install
```

启动前端开发服务器：

```bash
npm run dev
```

启动 Tauri 开发模式：

```bash
npm run tauri dev
```

构建前端：

```bash
npm run build
```

打包桌面应用：

```bash
npm run tauri build
```

清理 Rust/Tauri 构建缓存：

```bash
npm.cmd run clean
```

在 Windows PowerShell 中如果 `npm run clean` 被执行策略拦截，可以使用上面的 `npm.cmd run clean`。

---

## English

ClipForge is a desktop clipboard history manager built with Tauri, React, TypeScript, and Rust. It lives in the system tray, records text and image clipboard entries in the background, and opens a compact searchable window through a global shortcut.

### Purpose

When coding, writing, or collecting references, it is common to copy snippets, URLs, emails, colors, screenshots, and plain text repeatedly across applications. ClipForge turns those temporary clipboard values into searchable, groupable, and quickly pasteable history items.

### Features

- Clipboard history: monitors the system clipboard and stores text and image entries.
- Global shortcut: shows or hides the main window with `CmdOrCtrl+Shift+V` by default.
- Smart classification: detects URLs, emails, code snippets, colors, images, and plain text.
- Search and pagination: supports keyword search and infinite scrolling.
- Click to paste: writes a selected history item back to the clipboard and pastes it into the previous app.
- Image support: saves clipboard images and restores them from history.
- Custom groups: create, rename, delete, colorize groups, and move records into groups.
- Retention rules: configure maximum item count and maximum retention days; grouped records are protected from automatic cleanup.
- System tray: close hides the window to tray; tray menu can show the window or quit the app.
- Settings: configure autostart, global shortcut, database path, retention limits, and groups.

### Architecture

```text
ClipForge
├─ Frontend: React + TypeScript + Vite
│  ├─ src/App.tsx        Main UI, settings, search, groups, history list
│  ├─ src/App.css        Desktop window styling
│  └─ src/shortcuts.ts   Default shortcut constants
├─ Desktop Runtime: Tauri 2
│  ├─ src-tauri/src/lib.rs       App bootstrap, plugins, command registry
│  ├─ src-tauri/src/clipboard.rs Clipboard watcher, copy/paste, image loading
│  ├─ src-tauri/src/db.rs        SQLite storage, migrations, classification, groups, cleanup
│  ├─ src-tauri/src/shortcut.rs  Global shortcut registration and window positioning
│  ├─ src-tauri/src/tray.rs      System tray and window hide behavior
│  ├─ src-tauri/src/config.rs    External JSON configuration
│  └─ src-tauri/src/autostart.rs Autostart integration
└─ Storage
   ├─ SQLite: clipboard_items, custom_groups, config
   └─ Images: clipboard images stored as PNG files
```

The frontend handles interaction, filtering, settings, and rendering. The Rust backend handles native desktop capabilities such as clipboard access, global shortcuts, tray behavior, SQLite persistence, image file management, and simulated paste. The two layers communicate through Tauri commands.

### Storage

By default, ClipForge stores its SQLite database at `db/clipforge.db` next to the application executable. Clipboard images are stored under `images/` in the same database directory. A custom database path can be selected from the settings page. External configuration is saved as `clipforge.json` next to the executable.

### Development

Requirements:

- Node.js
- Rust stable toolchain
- Tauri system dependencies for your platform

Install dependencies:

```bash
npm install
```

Start the frontend development server:

```bash
npm run dev
```

Start Tauri development mode:

```bash
npm run tauri dev
```

Build the frontend:

```bash
npm run build
```

Build the desktop app:

```bash
npm run tauri build
```

Clean Rust/Tauri build artifacts:

```bash
npm.cmd run clean
```

If PowerShell blocks `npm run clean` because of script execution policy, use `npm.cmd run clean` on Windows.
