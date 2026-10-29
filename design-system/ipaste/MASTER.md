# iPaste 设计系统 · Hairline Ledger

> 权威设计参考。与 `AGENTS.md` 的「界面系统」一节配合使用：
> AGENTS.md 的桌面适配规则优先，本文件给出具体令牌、几何与状态决策。

## 1. 方向

**Hairline Ledger（发丝账本）**——安静的精密仪器。层级来自 1px 规则线、固定栅格、
对齐和单侧 2px 青绿脊线；阴影只留给真正悬浮的层（菜单 / 对话框 / toast）。
列表是平铺账本行，不是一行一张圆角卡片。

- 产品气质：安静、快速、聚焦，具备工具软件的可靠感。
- 记忆点：选中行、设置侧栏激活项、激活对话框页签共用同一条 2px 青绿脊线，
  配合等宽元数据形成「精密仪器」细节。
- 不做：装饰性渐变光斑、嵌套卡片、悬停位移、第二强调色、Web 字体。

## 2. 令牌

令牌全部定义在 `src/styles/theme.css`（`:root` 亮色，`html.dark` 暗色）。
组件样式只引用变量，不写原始色板与魔法数值。

### 2.1 表面与层级

| 令牌 | 用途 |
|---|---|
| `--bg-app` | 窗口底、边缘缝隙 |
| `--surface` | 导航带、列表、检查器、卡片 |
| `--surface-inset` | 搜索框、图标底座、kbd、代码井 |
| `--surface-hover` | 行 / 控件悬停 |
| `--row-selected` | 选中行染色 |
| `--surface-glass` | 菜单、对话框、toast |
| `--surface-code` | 文本 / 代码井 |
| `--backdrop` / `--backdrop-blur` | 模态遮罩 |

规则：流内表面只用 `--border-hairline` 分隔，不投影；`--shadow-pop/dialog/toast`
只服务浮层；`--shadow-xs` 只给分段控件滑块这类 1px 抬升控件。禁止卡片套卡片。

### 2.2 文字

| 令牌 | 亮色 | 暗色 | 说明 |
|---|---|---|---|
| `--text-1` | `#17181c` | `#ededef` | 主文字 |
| `--text-2` | `#5b5f70` | `#9a9eb0` | 必要元数据（≥6:1） |
| `--text-3` | `#666b7e` | `#7d8296` | 次要文字（≥4.5:1） |
| `--text-faint` | `#9ba0b2` | `#555a6d` | 装饰 / 禁用，不承载信息 |

### 2.3 字阶与字重

`--text-2xs` 10px / `--text-xs` 11px / `--text-sm` 12px / `--text-md` 13px /
`--text-lg` 14px / `--text-xl` 16px。字重只用 `--fw-regular/medium/semibold`
（400/500/600）；14–16px 用 `-0.01em` 字距；数字元数据 `tabular-nums`。

### 2.4 青绿与状态色

| 令牌 | 亮色 | 暗色 | 用途 |
|---|---|---|---|
| `--accent` | `#0d9488` | `#14b8a6` | 焦点、脊线、激活态 |
| `--accent-strong` | `#0f766e` | `#2dd4bf` | 青绿文字 / 图标 |
| `--accent-fill` | `#0f766e` | `#14b8a6` | 主按钮底色 |
| `--on-accent-fill` | `#ffffff` | `#0c0d10` | 主按钮文字（均 ≥4.5:1） |
| `--accent-soft` / `--accent-soft-2` | 10% / 16% | 14% / 22% | 选中与激活底色 |
| `--border-accent` | 36% | 45% | 青绿描边 |
| `--danger` / `--danger-border` / `--on-danger` | — | — | 破坏性操作、错误 |
| `--warning-text` / `--warning-border` | — | — | 警告文案（不直接用 `--warning` 写字） |
| `--success-border` | — | — | 就绪徽章描边 |

强调色纪律：青绿只用于焦点、选中脊线、激活胶囊、主操作、回车提示。
橙 / 红只用于破坏性操作与错误。分类色是数据，只作为 6–8px 圆点 / 色块出现，
通过 `src/lib/categoryColors.ts` 的 `categoryColorVars()` 注入
`--category-color / --category-on-color / --category-soft / --category-ring`。

### 2.5 几何与动效

- 圆角：`--r-xs 3px` / `--r-sm 4px` / `--r-md 6px` / `--r-lg 8px`（控件与卡片上限）/
  `--r-xl 12px`（对话框）/ `--r-pill` / `--window-radius 14px`（仅无边框窗口）。
- 控件刻度：`--control-sm 26px` / `--control-md 32px` / `--control-lg 36px` /
  `--row-h 50px` / `--header-h 44px` / `--footer-h 36px` / `--spine-w 2px`。
- 间距：4px 基网格（`--sp-1…--sp-8`）。
- 动效：`--dur-fast 120ms` / `--dur-base 160ms` / `--dur-slow 200ms`，
  缓动 `--ease-out`。悬停 / 选中只换背景与颜色，不位移不缩放。
  `prefers-reduced-motion` 下所有循环动画显式 `animation: none`。

## 3. 主面板几何

- 分栏：`grid-template-columns: minmax(0, 1fr) clamp(264px, 44%, 300px)`。
- 账本行：50px（`--row-h`），0 行距，行间 1px 发丝线；560×620 一屏约 9 条。
- 搜索行：内边距 `10px 12px 8px`，输入 36px、`--r-md`、inset 底。
- 底栏：36px，11px 文字，kbd 18px 等宽。
- 窄窗（≤639px）：隐藏 Paste 文字只留 `↵`，分类标签最大 4.5rem，检查器 meta 单列。

## 4. 窗口差异

| 窗口 | 装饰 | 处理 |
|---|---|---|
| 主面板 | 无边框 | `--window-radius` + `clip-path` |
| 设置 | **原生标题栏** | 不要圆角 / 裁切 / 边框，避免四角缝隙 |
| 查看器 / LAN 同步 | 无边框 | 保留 `--window-radius` + `clip-path` |
| OCR 遮罩 / 结果 | 无边框 | 遮罩使用 `--overlay-base / --overlay-scrim / --on-overlay`（主题无关） |

## 5. 测试契约

以下类名与 DOM 顺序被单测依赖，重构时不要改名或重排：
`.raycast-left-pane`、`.clip-mini-card` / `-selected`、`.clip-card-dragging`、
`.clip-card-drop-*`、`.clip-card-delete-confirming`、`.type-filter-option` / `-active`、
主面板搜索图标按钮顺序（更新 / 追加复制 / OCR / 同步 / 设置 / 关闭）。

## 6. 视觉稿

- [mockups/main-panel.html](./mockups/main-panel.html) — 主面板亮 / 暗、560 / 720、
  默认 / 选中 / 颜色 / 图片 / 空态 / 搜索回退 / 错误横幅 / 右键菜单 / 焦点环 / 删除确认。
  纯 HTML/CSS，无外部依赖；位于 `src` 之外，不参与 Tailwind 扫描与打包。

## 7. 清理记录

- 删除 `.app-shell`、`.top-bar*`、`.search-box*`、`.main-content*`、`.clip-card-grid`、
  `.rail-item*`、`.fallback-group*`、`.raycast-filter-tabs*`、`.raycast-pill-tab*`。
- 删除 `.quick-preview*`、`.detail-pane*`、旧 `.clip-card-type-*` / `.clip-card-main` /
  `.clip-preview-*` / `.clip-metric-badge` / `.clip-expand-button` 等死样式与
  `QuickPreviewPanel.vue` 孤儿组件。
- 删除 Tailwind 原始色板（`slate/red/teal/blue/emerald/amber/violet/white/black`）
  与 `dark:` 变体，移除 `theme.css` 的 `html.dark --color-*` 反转块。
- 删除零引用阴影 / 光晕令牌（`--shadow-selected`、`--shadow-keycap`、
  `--glow-top-hairline`、`--radar-wave`）。
- 设置窗去掉窗口圆角 / `clip-path` / 外框；侧栏激活改 2px 脊线；分区面板改为
  发丝线分组；合并同视觉的分区图标类；删除不可达的 520px 断点。
- 面板入场改为由 `panelVisibilityChanged` 可见性驱动（180ms，位移 4px），
  不再依赖 `v-if` 重挂载；减少动效下关闭 `pulse-danger`、`skeleton-pulse`、
  `update-button-breathe`、`image-ocr-scan`、`glow-pulse`。
