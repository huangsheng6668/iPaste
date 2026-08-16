# iPaste 代理指南

## 产品方向

iPaste 是一款本地优先的 macOS 和 Windows 托盘剪贴板管理器。当前应用采用类似 Paste 的工作流：后台捕获剪贴板、以键盘为主的浮动历史面板、快速搜索、用于保存片段的分类，以及把选中的片段直接粘贴回之前聚焦的应用。

## 当前能力

- 将 macOS 和 Windows 作为一等桌面目标平台支持。
- 以托盘应用形式运行，并提供全局快捷键。
- 自动捕获本地文本剪贴板历史。
- 使用 SQLite 在本地存储历史记录和分类。
- 支持用户创建、重命名、删除和选择分类。
- 支持用户手动把历史片段加入分类。
- 支持搜索历史记录和分类片段。
- 支持通过写入系统剪贴板并触发平台粘贴快捷键来应用片段。

完整历史同步、账户认证、附件同步、富 HTML 保留和高级来源应用识别仍需按产品优先级谨慎推进。

## 技术栈

- 使用 Tauri v2 构建桌面外壳和原生系统集成。
- 使用 Vue 3，并采用 `<script setup lang="ts">`。
- 通过 Vite 插件使用 Tailwind CSS v4。
- 使用 Pinia 管理共享界面和数据状态。
- 使用 lucide-vue-next 提供图标。不要把表情符号当作界面图标。
- 使用 Rust 命令实现持久化存储、托盘行为、全局快捷键、剪贴板轮询和粘贴自动化。

## 界面系统

权威设计参考从 `design-system/ipaste/MASTER.md` 开始，该文档由 `ui-ux-pro-max` 生成；以下桌面应用适配规则优先级更高。

- 产品气质：安静、快速、聚焦，具备工具软件应有的可靠感。
- 布局：紧凑的应用外壳、键盘优先的搜索、密集且易扫描的片段卡片。
- 配色：以中性应用表面为主，使用青绿色表示焦点和操作状态；橙色只用于破坏性操作或高强调动作。
- 字体：默认使用系统界面字体栈，以获得原生桌面体验；标题保持紧凑。
- 圆角：卡片和控件最大 8px，除非平台浮窗表面需要更柔和的圆角。
- 动效：过渡时长保持在 150 到 200 毫秒；尊重减少动态效果设置。
- 可访问性：每个可交互元素都需要清晰的焦点状态和可预测的 Tab 顺序。
- 不要在应用外壳中加入营销式首屏。
- 不要使用装饰性渐变光斑、嵌套卡片，或会造成布局跳动的悬停变换。

## 架构规则

- 原生专属行为保留在 Rust 中，并通过小而清晰的 Tauri 命令暴露。
- Vue 组件尽量保持展示职责；共享数据放在 Pinia store 中。
- 自动剪贴板历史和用户保存到分类的条目是两个不同的产品概念。
- 不同步自动历史。未来只有用户保存的分类内容可以进入同步范围。
- 分类条目以快照形式持久化，确保它们不会因为历史清理而丢失。
- 按内容哈希对捕获到的文本去重。
- 避免没有明确迁移路径的破坏性文件或数据库迁移。

## Rust 模块布局

`src-tauri/src/` 按领域拆分为独立模块：

- `lib.rs`：Tauri 构建入口（`run()` 组合根）与跨模块共享常量。新增代码按模块归属，不要往 lib.rs 堆；lib.rs 不做 glob 再导出，其他模块一律用显式路径（如 `crate::util::now`）引用。
- `models.rs`：命令和模块共享的结构化 serde 数据模型（含 ts-rs TS 导出注解）。
- `error.rs`：`AppError` 统一错误契约（code/message/params），全部 Tauri 命令返回 `Result<T, AppError>`；新增错误先加变体与 code。
- `events.rs`：前后端事件契约唯一来源（事件名常量 + payload 结构体 + events.ts 生成测试）；Rust 侧其他文件不得出现 `ipaste://` 字面量。
- `util.rs`：跨模块共享的纯函数辅助（哈希/剪贴板类型检测/预览、`clean_*` 入参校验清理、`now`、本地化文案）。
- `store.rs` + `store/`：SQLite 持久化。子模块按域拆分（clips/categories/settings/automations/sync/migrations/secrets/rows/test_support），统一 `xxx_with_conn` 事务模式。
- `clipboard.rs`：剪贴板捕获、规范化和写回。
- `cloud.rs`：自托管同步 API 客户端（store 侧调用 cloud，cloud 不依赖 store）。
- `ocr/`：图片 OCR——`mod.rs` 状态检测与调度、`installer.rs` Windows 资源安装器、`tesseract.rs` Windows 识别执行、`vision.rs` macOS Vision 管线。
- `window.rs`：面板/设置/放大窗口、原生面板行为和窗口定位（辅助窗口统一走 `show_auxiliary_window`）。
- `tray.rs`：系统托盘、菜单文案与菜单事件处理。
- `shortcut.rs`：全局快捷键注册与更新。
- `paste.rs`：目标应用激活与触发粘贴（含粘贴编排与快捷键投递）。
- `automation.rs`：自动化动作的进程执行与事件流。
- `commands.rs`：向 UI 暴露模块函数的薄 Tauri 命令层（业务编排在域模块中）。
- `lan_sync/`：LAN 同步（protocol/crypto/session/server/client/commands/port/pair_guard 分层，v4 握手协议见各文件头注释）。

新增后端功能时：数据模型进 `models.rs`，持久化进 `store/`，平台能力进对应域模块，命令层保持薄壳。跨平台代码用 `#[cfg(target_os = "...")]` 包裹，并保持逐平台 import 与代码块一致。

## Rust 规范

- 对命令载荷使用带 `serde` 的结构化数据模型。
- 将 SQLite 访问封装在小型存储层后面。
- Tauri 命令统一返回 `Result<T, AppError>`（`error.rs`，序列化为 `{code, message, params}`）；前端按 code 分支，不解析 message。
- 避免让面向界面的命令执行长时间阻塞操作。
- 平台特定的输入模拟应统一放在一个辅助函数后面。
- 在 macOS 上，需要说明直接粘贴依赖辅助功能权限。

## Vue 规范

- 使用组合式接口和 `<script setup lang="ts">`。
- 组件类型应使用本地 `types.ts` 模型。
- 优先使用计算状态，避免重复保存派生状态。
- 面板内的键盘快捷键应通过明确行为体现可发现性，不要使用大段说明文案。
- 命令按钮以及分类、类型提示应使用 lucide 图标。

## 前端结构

- `stores/ipasteStore.ts`：数据快照缓存与 CRUD 包装；`stores/lib/` 为纯函数库（ordering/selection/settings 清洗/automationFilter/automationTransfer，均带单测）；`stores/uiStore.ts` 为 toast 等瞬态 UI 状态。
- `composables/`：按功能簇拆分——useAppEvents（全局事件接线）、useQuickPreview、useAutomationFlow、useClipContextMenu、usePanelKeyboard、useClipListScroll、useDragSort（两处排序共用的指针拖拽引擎）、useLanSync、useUpdater 等。
- App.vue 只保留多窗口路由、面板布局骨架与 composable 接线；新增交互逻辑先进 composable，展示组件保持无业务状态。
- 错误双通道：加载失败走 store.error 持久横幅；动作失败走 uiStore.pushToast（ErrorToast.vue 渲染）。
- `lib/env.ts` 是 isTauri 唯一来源；事件名一律用 `types/generated/events` 的 IPASTE_EVENTS。

## 验证

在声明重要功能完成前，运行：

- `npm run build`
- `cargo check --manifest-path src-tauri/Cargo.toml`

涉及界面工作时，需要检查桌面和窄视口表现，并确认文本不会重叠或溢出控件。

## 发布流程

发布新版本时按以下顺序操作：

- 先确认当前分支和工作区状态：`git status --short --branch`。
- 运行 `npm run release`，根据提示输入新版本号。例如当前版本是 `0.1.11` 时，发布补丁版输入 `0.1.12`。
- 脚本会同步更新 `package.json`、`package-lock.json`、`src-tauri/tauri.conf.json` 和 `src-tauri/Cargo.toml`；如果 `src-tauri/Cargo.lock` 因版本号变化产生改动，也一并纳入提交。
- 发版前至少运行 `npm run build`。涉及 Rust 或 Tauri 原生逻辑时，再运行 `cargo check --manifest-path src-tauri/Cargo.toml`。
- 暂存并提交本次发布相关改动：`git add -A`，然后 `git commit -m "chore: release v版本号"`，例如 `git commit -m "chore: release v0.1.12"`。
- 创建发布标签：`git tag v版本号`，例如 `git tag v0.1.12`。
- 推送主分支和标签：`git push origin main`，然后 `git push origin v版本号`。
- 远端发布工作流由 `v*` 标签触发。推送标签后，需要检查 GitHub Actions 运行结果和发布产物。
