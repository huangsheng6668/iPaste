# iPaste

> 本地优先、键盘友好的桌面剪贴板管理器。把临时复制变成可搜索、可整理、可再次使用的工作流。

**语言:** [English](README.md) | 简体中文 | [日本語](README.ja.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md)

iPaste 常驻系统托盘，在本机记录剪贴板历史。你可以用全局快捷键快速唤出面板，搜索旧内容，回车粘贴，也可以把常用片段保存到分类里长期复用。

它适合经常在聊天、浏览器、终端、设计工具、笔记和代码编辑器之间切换的人：复制过的链接、命令、色值、提示词、回复模板、截图文字，不必再靠临时文件或反复翻聊天记录找回来。

![iPaste 桌面预览](docs/assets/ipaste-app-preview.jpg)

## 特性

- 本地优先：剪贴板历史存储在当前设备的本地 SQLite 数据库中。
- 快速唤出：默认使用 <kbd>Command</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> / <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> 打开面板，也可以在设置里修改。
- 多类型记录：支持文本、链接、颜色、HTML 片段、图片和文件类剪贴板内容。
- 搜索与键盘操作：面板为快速查找和回车粘贴优化，适合高频使用。
- 保存分类：把常用片段保存为分类条目，用于代码片段、命令、地址、回复模板和提示词。
- 图片查看：支持图片预览、缩放、旋转、复制回剪贴板，以及 OCR 文本提取。
- 追加复制：可把多次文本复制临时合并为一个片段，适合收集多段资料。
- 跨设备同步：两台设备交换一次性邀请票据即可跨互联网互信，剪贴板内容端到端加密直传（QUIC + NAT 打洞，打洞失败经中继转发密文），无需云账号；支持多设备管理、撤销与自动重连。
- 快捷动作：把 shell 命令保存为一键执行的面板动作，支持可选确认、输出流式展示和 JSON 导入导出。
- 可配置偏好：支持历史保留时长、面板布局、打开行为、全局快捷键、语言和 OCR 模式设置。
- 可选自托管同步：只同步保存分类和已保存的类文本内容，原始剪贴板历史始终留在本机。
- 签名更新：内置 Tauri 更新器支持，发布产物可通过 GitHub Releases 或 Cloudflare R2 分发。

## 下载

前往 [Releases](https://github.com/iPaste-app/iPaste/releases/latest) 下载最新版本。

当前发布流程面向：

| 平台 | 架构 | 说明 |
| --- | --- | --- |
| Windows | x64 | 使用系统 WebView2 Runtime；没有时需要先安装。 |
| macOS | Apple Silicon | 自动粘贴需要授予辅助功能权限。 |
| macOS | Intel | 自动粘贴需要授予辅助功能权限。 |

Linux 暂未作为正式目标平台。Tauri 具备跨平台能力，但当前仓库主要验证 macOS 和 Windows。

### macOS：提示「已损坏，无法打开」

iPaste 的 macOS 安装包目前尚未经过 Apple 公证，从浏览器下载后可能被 Gatekeeper 拦截，提示「iPaste 已损坏，无法打开。你应该将它移到废纸篓」。

这不是文件损坏，应用可以正常使用。推荐用一键安装脚本（自动拷贝应用到 /Applications 并清除隔离标记）：

```bash
bash <(curl -fsSL https://github.com/huangsheng6668/iPaste/releases/latest/download/install-macos.sh)
```

也可以手动把应用拖入 /Applications 后执行：

```bash
xattr -dr com.apple.quarantine /Applications/iPaste.app
```

已安装的用户请优先使用应用内的「检查更新」——更新包不受 Gatekeeper 限制。

### macOS 权限说明

iPaste 在 macOS 上需要两个不同的权限，请在「系统设置 → 隐私与安全性」中分别授予：

- **辅助功能**：用于自动粘贴（模拟按键）。
- **屏幕录制**：用于截图识别文字（OCR）。注意它与辅助功能是两个独立权限，只勾辅助功能时截图 OCR 无法使用。

勾选权限后需要**完全退出并重启 iPaste** 才会生效。由于安装包未做签名，**每次升级 iPaste 后权限可能会失效**（表现为设置里勾选着但应用提示未授权）：请在对应权限列表中先移除 iPaste，再重新添加并勾选，然后重启应用。

## 快速上手

1. 启动 iPaste，它会常驻托盘并开始监听剪贴板。
2. 像平时一样复制文本、链接、颜色或图片。
3. 按 <kbd>Command</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> 或 <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> 唤出面板。
4. 搜索、选择条目，然后按回车粘贴回当前应用。
5. 对长期复用的内容，保存到分类并按自己的工作流整理。

macOS 上的自动粘贴需要辅助功能权限。Windows 上的图片 OCR 需要在设置中下载 PaddleOCR 模型。

## 隐私与数据

iPaste 的默认模型是本地优先。

- 自动捕获的剪贴板历史不会被上传或同步。
- 本地数据保存在系统应用数据目录下的 SQLite 数据库中。
- 跨设备同步在你自己的设备间跨互联网直接传输内容。互信由一次性邀请票据建立，传输由 QUIC TLS 端到端加密（NAT 打洞失败时中继仅转发密文），无需云账号。
- 启用云同步时，只同步分类和已保存的文本、链接、颜色、HTML 内容。
- 图片和文件片段目前不在云同步载荷中。
- 云同步需要你自己配置 API 地址和 API Key。
- 更新器会在安装前校验已签名的发布产物。

如果你的剪贴板经常包含密码、密钥、客户资料或公司内部内容，请在使用任何剪贴板管理器前确认团队安全规范。

## 平台支持

| 平台 | 状态 | 备注 |
| --- | --- | --- |
| macOS | 已支持 | OCR 使用系统 Vision 框架；自动粘贴需要辅助功能权限。 |
| Windows | 已支持 | OCR 使用可下载的 PaddleOCR 模型。 |
| Linux | 暂未支持 | 当前没有正式发布和完整验证。 |

## 技术栈

- Tauri 2：桌面外壳、托盘、窗口、更新器和系统集成。
- Rust：剪贴板捕获、SQLite 存储、全局快捷键、粘贴自动化、OCR 管线和同步编排。
- Vue 3、TypeScript、Pinia、Vite、Tailwind CSS 4：应用界面。
- `rusqlite`：本地 SQLite 持久化。
- Cloudflare Pages/Workers 兼容 API：可选同步服务。

## 开发

### 环境要求

- Node.js 22 或更高版本。
- npm 10 或更高版本。
- Rust stable 工具链。
- 当前操作系统所需的 Tauri 2 平台依赖。

macOS 开发需要 Xcode Command Line Tools。Windows 开发需要 Microsoft C++ Build Tools；如果系统没有 WebView2 Runtime，也需要一并安装。

### 安装依赖

```bash
npm install
```

### Web 预览

```bash
npm run dev
```

浏览器预览会在原生 Tauri API 不可用时使用模拟数据，适合做界面开发，但不会捕获真实系统剪贴板。

### 桌面开发

```bash
npm run tauri dev
```

### 构建

```bash
npm run lint        # ESLint 检查
npm test            # Vitest 前端单测
npm run build       # 类型检查（vue-tsc）+ Vite 生产构建
npm run tauri build # 桌面安装包
```

快速检查原生编译：

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

### 共享类型

`src/types/generated/` 中的 TypeScript 绑定由 ts-rs 从 Rust 生成。修改 `models.rs` 共享模型或 `events.rs` 事件定义后，请重新生成并提交——CI 会校验新鲜度。

```bash
npm run gen:types
```

## 项目结构

```text
.
├── src/                  # Vue 应用：组件、composables、Pinia store 与前端 API 封装
├── src-tauri/            # Tauri 配置和 Rust 桌面后端
│   └── src/              # Rust 后端模块（见下表）
├── scripts/              # 发布、版本和更新器分发工具
├── docs/                 # 运维文档和项目笔记
├── key/                  # 公开更新器公钥；私钥不得进入 git
└── .github/workflows/    # CI 与已签名桌面构建的发布工作流
```

`src-tauri/src/` 下的 Rust 后端按领域拆分为小模块：

| 模块 | 职责 |
| --- | --- |
| `lib.rs` | Tauri 构建入口（`run()` 组合根）与共享常量 |
| `models.rs` | 命令和模块共享的结构化 serde 数据模型（经 ts-rs 导出为 TypeScript） |
| `error.rs` | `AppError`：统一的命令错误契约（`{code, message, params}`） |
| `events.rs` | 前后端事件名与 payload 的唯一来源；生成 `src/types/generated/events.ts` |
| `util.rs` | 共享纯函数：哈希、剪贴板类型检测、`clean_*` 校验清理、本地化文案 |
| `store.rs` + `store/` | SQLite 持久化，按域拆分子模块（clips/categories/settings/automations/sync/migrations/secrets） |
| `clipboard.rs` | 剪贴板捕获、规范化和写回 |
| `cloud.rs` | 自托管同步 API 客户端 |
| `lan_sync/` | 跨设备同步（v5）：iroh QUIC 传输、一次性邀请票据、设备身份与信任表、多设备会话注册、配对防护 |
| `ocr/` | 图片 OCR：资源安装器与状态检测（Windows）、PaddleOCR 执行（Windows）、Vision 管线（macOS） |
| `window.rs` | 面板/设置/放大窗口、原生面板行为和窗口定位 |
| `tray.rs` | 系统托盘、菜单文案与菜单事件处理 |
| `shortcut.rs` | 全局快捷键注册与更新 |
| `paste.rs` | 目标应用激活与触发粘贴 |
| `automation.rs` | 快捷动作的进程执行与事件流 |
| `commands.rs` | 向 UI 暴露域模块的薄 Tauri 命令层 |

## 工作原理

### 剪贴板捕获

Rust 后端在后台监听系统剪贴板，对受支持的内容进行规范化，写入 SQLite，并把更新事件发送给 Vue 面板。类文本片段会按内容哈希去重。图片片段会作为本地应用数据资源保存，并通过 Tauri 资源协议渲染。

### 应用片段

从 iPaste 粘贴时，应用会先把选中的片段写回系统剪贴板，再触发平台粘贴快捷键。macOS 上的直接粘贴步骤需要辅助功能权限。

### 保存分类

历史片段和保存分类条目是两个不同概念。历史条目会根据保留策略过期；保存分类条目是用户明确保存的快照，会一直保留到用户删除为止。

### 云同步

桌面应用可以在偏好设置中配置 API 地址和 API Key，连接到自托管的 iPaste 同步 API。同步范围包括分类和已保存的类文本分类条目。同步服务源码待就绪后开源。

### 跨设备同步

两台 iPaste 通过一次性邀请票据配对：一台生成邀请，另一台凭票据加入，双方确认后互信。设备间经 QUIC 跨互联网直连（NAT 打洞失败时由中继转发密文），片段与整组分类在设备间端到端加密直传，接收端不存在的分类会自动创建。已配对设备可随时管理或撤销，断线后会自动退避重连。

### 快捷动作

快捷动作是保存的 shell 命令，显示在独立的面板分类里。一键执行、可选先确认、在详情窗格查看流式输出，并可通过 JSON 导入导出在机器间分享。

### 图片 OCR

macOS 使用系统 Vision 框架。Windows 使用可在应用偏好设置中安装的 PaddleOCR 模型。

## 贡献

欢迎提交 Issue、讨论想法或发起 Pull Request。

提交 Pull Request 前请至少运行：

```bash
npm run lint
npm test
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
```

在 Windows 上构建 Rust 后端需要 bindgen 所需的 libclang（PaddleOCR 引擎依赖）。可用 `choco install llvm` 安装，或 `pip install libclang` 并把 `LIBCLANG_PATH` 指向 Python site-packages 下的 `clang/native` 目录。

如果改动涉及共享 Rust 模型或事件，还需要运行 `npm run gen:types` 并把再生成的绑定一并提交。

请保持项目本地优先、尊重隐私，并谨慎处理任何会同步用户数据的改动。较大的功能建议先开 Issue 讨论边界和交互。

## 许可证

本项目采用 Apache License 2.0 许可。详见 [LICENSE](LICENSE) 和 [NOTICE](NOTICE)。

再分发时请保留许可证、版权和 NOTICE 信息；修改过的文件需要说明改动。
