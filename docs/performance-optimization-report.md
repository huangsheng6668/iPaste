# iPaste 性能优化分析报告

> 分析日期：2026-10-15 · 范围：Rust 后端（src-tauri/）、Vue 前端（src/）、构建配置
> 方法：四路并行代码走读 + 关键结论逐条源码抽查验证（文中行号均已核对）

## 总览：优先级最高的四件事

| # | 问题 | 位置 | 收益 | 成本 |
|---|------|------|------|------|
| 1 | 剪贴板轮询每 700ms 无条件全量读取，无变化门控 | `capture/watcher.rs:51-53` | 消除 ~99% 后台读取，图片常驻时不再持续 PNG 重编码 | 极小（约 10 行） |
| 2 | `visibleItems` 每次重算重建全部条目对象 → 全列表重渲染 | `stores/ipasteStore.ts:68-80` + `components/ClipCard.vue:35-71` | 单次捕获的 O(n) 重渲染 + n 次重量级 computed 降为 O(1) | 小 |
| 3 | mocr sidecar 每个解码步完整克隆 encoder hidden states | `bin/mocr_engine.rs:201,211,216` | 每张图消除约 180MB 分配 churn | 小 |
| 4 | Cargo.toml 无 `[profile.release]` | `src-tauri/Cargo.toml` | 体积 -10~30%，运行时 +2~8% | 极小 |

以下按优先级分组详述。

---

## P0 — 高收益、低成本（建议立即做）

### 1. 剪贴板 watcher：加 change-id 一级门控

**位置**：`src-tauri/src/capture/watcher.rs:44-125`（轮询体）、`src-tauri/src/clipboard.rs:76-118`

**现状**：每轮循环先取 `before_change_id`，然后**无条件**调用 `read_current_clipboard()`，事后才用 before/after 一致性判定：

```rust
// watcher.rs:51-55
let before_change_id = clipboard_change_id();
match read_current_clipboard() {
    Ok(ClipboardRead::Item(item)) => {
        let after_change_id = clipboard_change_id();
```

`read_current_clipboard()` 每次都会：试 `get_image()`（图像剪贴板下读取完整 DIB 并 `into_owned()` 拷贝）→ PNG 重新编码整张图 → SHA-256 → 再试 file_list → `get_text()`。剪贴板里放着图片时，**每 700ms 就把整图重新解码拷贝 + PNG 编码 + 哈希一遍**，仅为了得出"已捕获过"（`AlreadyCaptured`，watcher.rs:103-105）。文本路径每轮也有多次 OpenClipboard/CloseClipboard。

**修复**：平台原生提供廉价的变化序号（`GetClipboardSequenceNumber`，已在 `clipboard.rs:250-254` 封装），把它作为一级门控——未变化则整轮跳过：

```rust
let current_id = clipboard_change_id();
let last = last_clipboard_change_id.lock().ok().and_then(|g| *g);
if current_id.is_some() && current_id == last {
    thread::sleep(Duration::from_millis(700));
    continue; // 内容未变，跳过完整读取
}
```

marker 不变量已由 iPaste 自身写剪贴板（`remember_current_clipboard_marker`）和清空遗忘维护，门控安全。**预期影响：高**——消除约 99% 的读取，彻底消除图片常驻剪贴板时的持续 CPU/内存抖动（4K 图每轮数十至上百 ms）。

### 2. 前端：稳定 view item 对象 + Intl 单例化

**位置**：`src/stores/ipasteStore.ts:68-80`、`src/components/ClipCard.vue:35-71`、`src/lib/format.ts:11-31,60-93`

**现状（两个叠加问题）**：

a) `visibleItems` 每次重算都对**每一条** clip 做展开拷贝，产出全新对象：

```ts
// ipasteStore.ts:72
? clips.value.map((clip) => ({ ...clip, collection: "history" as const }))
```

b) 每张 `ClipCard` 的渲染路径上都是重量级 computed：

```ts
// format.ts:16 —— 每次调用新建 RelativeTimeFormat
const relativeFormatter = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
// format.ts:92 —— 每次新建 Segmenter，并把全部 grapheme 展开成数组再数长度
return segmenter ? [...segmenter.segment(text)].length : Array.from(text).length;
// format.ts:69 —— isCodeText 对整条 text 跑多行正则
```

**后果**：一次剪贴板捕获 → `clips` 数组更换 → `visibleItems` 全量重建（n 个新对象）→ 每张卡片拿到全新 `item` prop → 7 个 computed 全部失效 → n 次（Segmenter + 全量 grapheme 数组 + RelativeTimeFormat + 全文正则）。n=500 时单次事件几十到上百 ms 主线程占用，直接卡面板。

**修复**：
- 用 `WeakMap<ClipItem, ClipViewItem>` 缓存包装对象（或把 `collection` 拆成独立 prop 透传原引用），底层 clip 不变则 wrapper 不变；
- Segmenter / RelativeTimeFormat / DateTimeFormat 提为模块级单例（locale 变化时重建）；
- 计数改 `for (const _ of segmenter.segment(text)) count++`，不物化数组；
- `metricText`/`displayTime`/`iconComponent`/`isCodeText` 随 clip 缓存，clip 不变则字符串不变。

**预期影响：高**——这是单次 store 写入耗时的最大来源，修完倍率归零。

### 3. mocr sidecar：decoder 每步克隆 hidden states

**位置**：`src-tauri/src/bin/mocr_engine.rs:198-238`

**现状**：encoder 输出被 `to_vec()` 成 `Vec<f32>`（约 605KB），greedy 自回归循环（最多 299 步）里每一步 `encoder_hidden_states.clone()` 整份拷贝喂给 ONNX：

```rust
// mocr_engine.rs:216
"encoder_hidden_states" => Tensor::from_array((
    [1usize, hidden_len, hidden_dim],
    encoder_hidden_states.clone(),   // 每 token 步一份 605KB
))
```

单张图最多约 **180MB 的分配 + memcpy churn**，纯浪费。这是 Windows/macOS-aarch64 上 manga OCR 的主路径，每张图都命中。

**修复**：`ort::Value` 内部是 Arc 引用计数。一次性构造 `Tensor`/`Value` 存入变量，循环内对该 Value `.clone()`（自增引用计数）而非克隆底层 Vec。**预期影响：高**（manga 识别热路径）。

### 4. Cargo 发布配置缺失

**位置**：`src-tauri/Cargo.toml`（全文无 `[profile.release]`）

**现状**：走 cargo 默认值——`lto = false`、`codegen-units = 16`、`strip = "none"`、`panic = "unwind"`。868 个 crate 的依赖树（iroh/quinn/rustls/hyper）在 fat LTO 缺失下，体积与运行时双损失；不剥符号在 Windows 上通常多 5–15MB。

**修复**：

```toml
[profile.release]
opt-level = 3
lto = "thin"          # 体积/构建时间平衡；极限可 true
codegen-units = 1
panic = "abort"
strip = "symbols"
```

注意：`panic = "abort"` 需确认无依赖用 `catch_unwind`；`strip` 会丢崩溃栈符号（可另存未 strip 副本调试）。**预期影响：高**（体积 -10~30%，运行时 +2~8%；代价是 release 构建时间 +30~100%）。

---

## P1 — 高收益、中等成本

### 5. 列表无虚拟滚动，分页窗口无上限

**位置**：`src/components/ClipListPane.vue:163-193`、`src/stores/ipasteStore.ts:128-148, 234-243`

**现状**：`v-for` 全量渲染所有已加载卡片，无窗口化；触底无限追加（每页 20 条）。剪贴板管理器持有几千条历史很常见——几千个组件实例 + 数万事件监听同时挂载。另有一个行为缺陷：捕获路径有 `.slice(0, 120)` 截断（ipasteStore.ts:242），分页路径没有——滚过 500 条后复制一次，列表会突然被打回 120 条。

**修复**：卡片是定高迷你卡，适合自研虚拟滚动（只渲染视口 ±5~10 条 overscan，spacer 撑高度）；短期至少给已加载窗口设硬上限（200~300 条）+ 显式"加载更多"，并统一捕获路径与分页路径的窗口策略。**预期影响：高**（剪贴板场景最核心的扩展性问题）。

### 6. 搜索无防抖，本地全量过滤与后端重复

**位置**：`src/App.vue:635`、`src/stores/ipasteStore.ts:77-79`、`src/lib/clipSearch.ts:10-19`

**现状**：`@input` 直接写 store，无防抖；每次按键 → `visibleItems` 同步重算 → 对整个已加载窗口逐条 `toLowerCase()`（含完整 `text` 全文）过滤。query 归一化还放在每条目调用内部重复执行。滚过 500 条、平均 2KB 文本时，一次按键 = 数 MB 临时字符串分配，输入明显掉帧。且 history + 搜索态下后端 `search_with_fallback` 已按同一口径过滤（防抖 160ms 已存在于 `useClipListScroll.ts:74-80`），本地这一遍是纯浪费。

**修复**：搜索词加防抖（与 160ms 对齐）；history + 搜索态直接信任后端分页结果、跳过本地 filter；至少把归一化提到循环外；进阶可入库时预算一份小写搜索索引。**预期影响：高**。

### 7. SQLite 每次操作新建连接 + 设置读路径含 keyring 与 DELETE

**位置**：`src-tauri/src/store.rs:39-51`（调用点约 90 处）、`src-tauri/src/store/settings/write.rs:86-166`

**现状**：

```rust
// store.rs:39-50 —— 每次 store 操作都执行
let conn = Connection::open(&self.db_path)...;
conn.pragma_update(None, "journal_mode", "WAL")...;
conn.pragma_update(None, "foreign_keys", "ON")...;
conn.busy_timeout(...)...;
```

每次列表查询、每次捕获落库、每次设置读取（含每次 OCR 请求）都重新 open 文件 + 跑 3 条 pragma；rusqlite 无 prepared statement 缓存。更糟的是 `settings()` 读取路径里还做 keyring I/O 和一次 `DELETE FROM settings WHERE key = 'bigmodel_api_key'`（一次性迁移语义混进了热路径读函数，write.rs:140-142）。

**修复**：(a) 连接池（`r2d2_sqlite` 或自建 `Mutex<Vec<Connection>>` 空闲池），pragmas 仅首次设置；(b) bigmodel 遗留清理搬进 `Store::migrate()` 只跑一次；(c) `AppState` 缓存 `AppSettings`（`Arc<RwLock<AppSettings>>`），写时失效、读走内存。**预期影响：中高**（OCR 首响、面板打开延迟显著下降；消除读路径写库）。

### 8. 启动时主线程 block_on iroh endpoint 绑定

**位置**：`src-tauri/src/lib.rs:385-394`

**现状**：同步 setup 闭包内 `block_on(DeviceLinkRegistry::start(...))`——内部是 QUIC socket 绑定 + 中继 URL 的 DNS 解析 + 中继连接。之后才执行 `build_tray`、`register_app_shortcut`、`show_main_window`（lib.rs:425-437）。默认中继模式下慢网/首次启动可达数秒，期间**无托盘、无主窗口、无快捷键**，用户看到"应用卡住"。

**修复**：命令层已有对缺失 registry 的优雅处理（`commands.rs:31-37`），可把 registry 启动改为 `tauri::async_runtime::spawn` 后台执行、就绪后再 manage；`show_main_window`/`build_tray` 移到 registry 启动之前，让 UI 立即可见。**预期影响：高**（冷启动体验）。

### 9. LAN 图片接收：多次全量拷贝 + PNG 往返编解码全压在读任务上

**位置**：`src-tauri/src/lan_sync/session/apply.rs:58-59,175-179`、`src-tauri/src/clipboard.rs:324-331,353-375,386-423`

**现状**：一张 4K 图接收链路：payload(≤8MB) → `from_utf8_lossy().to_string()` 再拷一份 → base64 解码 → PNG 解码成 RGBA（~33MB）→ **重新编码回 PNG**（仅为算 hash 和落盘）→ 在读任务线程里直接写系统剪贴板。瞬时内存约 60MB；base64/PNG 解码/重编码是几百 ms 级 CPU 工作，全部同步跑在会话读任务上——期间该会话后续帧（含 Ping/Pong）全部停读。

**修复**：image 类型直接用 payload 字节做 base64 解码，避免 String 全量拷贝；payload 本身就是 PNG data-url 时，解码出的 PNG 字节直接用于 hash/落盘，跳过"解码成 RGBA 再重编码"的往返；解码与剪贴板写入移出读任务（`spawn_blocking`，批量场景注意保序）。**预期影响：中高**。

---

## P2 — 中等收益

### 10. COUNT 全表扫描 × 3 + `datetime()` 包裹使索引失效

**位置**：`src-tauri/src/store/clips.rs:49,97,214-246`、`src-tauri/src/store/migrations.rs:313`

**现状**：每次捕获（含重复命中路径）都执行 `SELECT COUNT(*) FROM clips` 仅为事件徽标；一次列表加载算 3 个 count（total/all/type counts，谓词相同）；`search_with_fallback` 又把同样的 count 算第二遍；`ORDER BY datetime(last_captured_at)` 和 `prune_expired` 的 `datetime()` 包裹使裸列索引无法用于排序/定位（RFC3339 UTC 字符串字典序即时间序，本不需要 `datetime()`）。

**修复**：三个 count 合并为单次扫描（`COUNT(*)` + `SUM(...)`）；搜索复用已算出的 total；去掉 `datetime()` 包裹让 `idx_clips_last_captured_at` 生效。**预期影响：中**（大库下列表/搜索/捕获路径接近常数级）。

### 11. 前端：`upsertClip` O(n) + 每次面板显示/每条设备事件全量 `load()`

**位置**：`src/stores/ipasteStore.ts:234-270`、`src/composables/useAppEvents.ts:60-67,117-125`

**现状**：`upsertClip` 每次捕获 O(n) 扫描 + 两次全量数组重建；`deviceClipReceived` / `deviceCategoryReceived` 每收到一条就触发一次全量快照 `load()`——批量同步 N 条 = N 次并发全量 IPC + N 次全量数组替换 + N 次整列表重渲染（经问题 2 放大），且互相 race；面板打开还是双重取数（`load()` 后 `activatePanelDefault()` 又把第一页拉一遍）。

**修复**：维护 `id → index` Map 做 O(1) 去重换位；load 做 in-flight 复用 + 200~300ms 合并窗口（或 dirty 标记）；`activatePanelDefault` 在快照刚加载过时跳过 `reloadClips`。**预期影响：中高**（设备同步场景尤其明显）。

### 12. Paddle 引擎 `Box::leak`：切换模式即泄漏旧模型

**位置**：`src-tauri/src/ocr/paddle.rs:194-231`

**现状**：引擎按 `"{mode}:{is_manga}"` 键缓存（这点是对的），但换键时旧引擎被 `Box::leak` 出的引用直接覆盖、**永不 drop**——用户在 fast/best、普通/manga 档位间每次切换泄漏一整套 det+rec MNN 模型（十几 MB 起），内存无上界增长。另外 `std::sync::Mutex` 跨越数秒的模型加载，两个模式首次并发 OCR 时第二个会占着 blocking-pool 线程干等锁。

**修复**：改 `Mutex<Option<(String, Arc<OcrEngine>)>>` 或 per-key `OnceCell`，替换时 drop 旧值；加载期间不持锁。**预期影响：中**。

### 13. automation：每行输出新建 SQLite 连接 + 日志 O(n²) 重写

**位置**：`src-tauri/src/automation.rs:149-175`、`src-tauri/src/store/automations.rs:146-175`、`src/composables/useAppEvents.ts:93-100`

**现状**：`drain_stream` 每行输出都 `store.connect()` 新连接 + `append_automation_run_output`（先 SELECT 整段累计日志、拼接截断后整段 UPDATE——N 行输出重写 O(N²) 字节）+ 向前端 emit 一个事件。命令类快捷动作（npm install、git clone）动辄数千行。前端侧每个 chunk 还 spread 整个 logs Map + 对最长 200KB 字符串拼接再 slice。

**修复**：drain 任务生命周期内复用一条连接；输出行批量聚合（50ms 窗口或 32 行一批）再 emit + 落库；日志改增量列或环形缓冲；前端 logs 按 runId 独立 ref + 原地变更。**预期影响：中**（chatty 命令场景）。

### 14. 同步 Tauri 命令在主线程执行阻塞操作

**位置**：`src-tauri/src/commands/mod.rs:34-60`、`src-tauri/src/commands/cloud.rs:41-44`、`src-tauri/src/commands/clips.rs:132-151`、`src-tauri/src/paste.rs:141`

**现状**：`get_snapshot` → `build_app_snapshot`（`prune_expired()` 全表扫描 DELETE + 3 次 COUNT + 全量分类 + 含 keyring 的 settings()）在主线程；`sync_cloud_now`/`test_cloud_settings` 阻塞 HTTP（12s 超时）；`apply_clip` → `paste_to_previous_app` 先 `thread::sleep(180ms)` 再合成按键——每次粘贴冻结 UI 约 200ms。大历史库（5k+）时快照/网络调用期间 webview 卡顿。

**修复**：改 `async fn` + 阻塞段包 `spawn_blocking`（项目在 `commands/screenshot.rs:7`、`commands/ocr.rs:111` 已有此模式）。**预期影响：中**。

### 15. 多显示器截图：所有整屏帧同时驻留 + 转换前整体 clone

**位置**：`src-tauri/src/capture/mod.rs:42,63-79,272-289`

**现状**：`frozen_frames: Vec<Option<RgbaImage>>` 在会话期间持有每一台显示器的整屏 RGBA 帧；写盘时又整体 clone 一份再转 RGB。3×4K 布局下帧数据约 100MB 全程驻留，clone 使峰值再翻倍；每次触发还写约 75MB 未压缩 BMP（选 BMP 是为避免 JPEG 软编码数秒卡顿，此取舍有注释记录，勿轻改）。

**修复**：去掉 clone——按行做 RGBA→RGB 转换直接喂 encoder；中长期可在遮罩展示后丢弃非光标所在显示器的帧（需权衡跨屏拖选）。**预期影响：中**（多屏 4K 用户内存峰值约减半）。

### 16. 构建：单 chunk 全量打包，6 种窗口共用同一 bundle

**位置**：`vite.config.ts`（无 build 段）、`src/App.vue:5-22`

**现状**：无 `manualChunks`、无 vendor 分离；settings / clip-viewer / ocr-overlay / ocr-result / lan-sync 全部静态导入。OCR overlay 和 clip-viewer 由全局快捷键即时拉起，每次出现前都要完整下载解析整个应用 JS 才渲染一个分支。

**修复**：`manualChunks` 拆 vendor（vue/pinia/lucide/@tauri-apps/api）；对非主窗口组件改 `defineAsyncComponent` 按 `windowKind` 懒加载，优先拆快捷键敏感的 OCR overlay。**预期影响：中**（辅助窗口启动解析时间可降 30–60%）。

### 17. 同步网络：重拨无 jitter + 启动同刻拨号 + 心跳 timer 被重置

**位置**：`src-tauri/src/lan_sync/registry/link.rs:23-43,110-170`、`src-tauri/src/lan_sync/registry.rs:223-227`、`src-tauri/src/lan_sync/session/mod.rs:188-253`

**现状**：(a) 启动时为每台已配对设备立即 spawn link_task 并立刻拨号——50 台 = 启动瞬间 50 个并发 QUIC 握手/中继连接；(b) 退避序列确定性无 jitter，共同事件（对端重启、网络闪断）会让所有设备重拨时刻同步对齐，形成周期性重试波；(c) 心跳 `sleep(30s)` 在每轮 select 迭代时重建——持续剪贴板同步（每 <30s 就有推送）时定时器永远被重置，应用层 Ping 一帧都发不出（死亡检测由 QUIC 传输层兜底，不构成正确性问题，但自愈语义失效）。

**修复**：退避加 ±25% jitter；启动时每设备首次拨号加随机初始延迟（0–5s）；心跳改 `tokio::time::interval`（循环外创建）。**预期影响：中低**（设备多/网络不稳定时明显；心跳改动一行、风险极低）。

---

## P3 — 零碎小项（顺手可做）

| 问题 | 位置 | 修复 |
|------|------|------|
| OpenAI 请求中数 MB base64 data URL 被 clone | `ocr/openai.rs:188` | `std::mem::take` move 进最后一条 user 消息 |
| `sort_paddle_lines` 列/行聚合 O(N²)，每比较一次全列重算均值 | `ocr/paddle.rs:102-187` | 每列缓存 (sum_cx, sum_w, len) 增量更新，比较 O(1) |
| 窗口定位每次 show 重复枚举显示器（2–3 次），托盘点击路径在主线程 | `window.rs:216-265`、`window/positioning.rs:259-284` | 一次 show 枚举一次，`tauri::Monitor` 传给两次定位复用 |
| 卡片缩略图无懒加载/异步解码，原图全量并发解码（卡片只显示 14px） | `components/ClipCard.vue:139-144` | 加 `loading="lazy"` + `decoding="async"`；中期后端生成缩略图变体 |
| 查看器编辑器统计每按键全量重算（Segmenter + 全量 grapheme） | `composables/useClipEditor.ts:43-45` | 同 P0-2 单例 + 迭代计数；统计值可加 150ms 防抖 |
| 拖拽 ghost 每 pointermove 写 reactive → 整列重渲染 | `composables/useDragSort.ts:136-139` | ghost 变换直接写 DOM style，不走 reactive |
| App.vue 三个分类聚合 computed 全量重建，其中一个 O(k²) 建组 | `App.vue:229-251` | 合并成一次遍历产出三张表；push 替代 spread |
| 设备列表事件连接抖动时洪泛 | `lan_sync/registry/link.rs:295-296,361-362` | 列表事件 200ms 合并窗口 |
| mocr 每次请求重复 fs 探测 sidecar/模型目录 | `ocr/mocr.rs:128-139` | 按"设置变更时失效"缓存探测结果 |
| tokio 被传递依赖拉回 near-full features（Cargo.toml 裁剪被 feature unification 击败） | `Cargo.toml:48` | cargo 机制固有；保留显式裁剪，用 `cargo tree -e features -i tokio` 定期审查 |
| Tailwind v4 默认扫描仓库根（含 8 个 README、docs/ 284KB） | `src/styles/main.css` | 加 `@source "../src";` 显式限定 |
| i18n 7 种语言全部静态打包（~200KB 源码） | `src/i18n/index.ts:4-10` | 非默认语言改动态 import |
| `.cargo/config.toml` 无链接器优化 | `.cargo/config.toml` | MSVC 加 `-C link-arg=/DEBUG:NONE`（dev 提速）；评估 lld-link |

---

## 验证过、确认没有问题的点

- **WAL 模式与 5s busy_timeout 已启用**；`content_hash` UNIQUE 索引、`category_items(category_id, sort_order)`、`automation_runs(automation_id, started_at DESC)` 索引齐全。
- **捕获事件是增量更新**（`useAppEvents.ts:40-42` → `upsertClip` prepend），不是全量重载；全量重载只发生在面板可见性与设备同步路径（见 P2-11）。
- **后端搜索有 160ms 防抖**（`useClipListScroll.ts:74-80`）；问题在本地过滤不过防抖（P1-6）。
- **Paddle 引擎有进程级缓存**（同键命中不重载）；**mocr sidecar 进程复用**（常驻、空闲 5 分钟回收、结果窗打开时预热），ONNX Session 只建一次——问题只在换键泄漏与 hidden states 克隆。
- **LAN 无广播风暴**：每设备独立单播 + 独立 link_task，捕获线程永不被网络阻塞（`fan_out_auto` 用 `try_send` 非阻塞入队，队列满即丢弃计数）；整组发送流式过容量 16 的 mpsc，有背压、内存有界。
- **图片解码有显式尺寸/内存上限**（`clipboard.rs:28-29,402-405`）；LAN 帧有 8MB 大小上限。
- **托盘菜单只建一次**，后续仅 `set_text` 更新单项；`handle_settings_menu` 已用 `thread::spawn` 避让主线程。
- **v-for key 稳定**（`ClipListPane.vue:166` 等）；查看器图片只解码一次，缩放/平移/旋转全走 CSS transform。
- `index.html` 无阻塞资源；`main.ts` 极简；devtools 未启用；主窗口先隐藏后显示。

---

## 建议执行顺序

1. **第一批（合计约 200 行改动，消除绝大多数日常浪费）**：P0-1 watcher 门控 → P0-2 前端对象稳定化 + Intl 单例 → P0-3 sidecar 克隆 → P0-4 release profile。前三项覆盖"后台轮询 CPU"和"每次 store 写入 O(n) 重渲染"两大日常热路径。
2. **第二批（扩展性）**：P1-5 虚拟滚动 → P1-6 搜索防抖 → P2-11 upsert/load 合并。解决几千条历史 + 快速连续复制的根本问题。
3. **第三批（I/O 与启动）**：P1-7 连接池 + 设置缓存 → P1-8 启动异步化 → P1-9 接收拷贝链。
4. **第四批（按需）**：P2 其余项 + P3 零碎项，随功能迭代顺手带入。

> 所有改动遵循 AGENTS.md 验证要求：前端改动跑 `npm run lint` / `npm test` / `npm run build`；Rust 改动跑 `cargo check` / `cargo test`；触及共享模型/事件契约另跑 `npm run gen:types` 并确认 `git diff --exit-code -- src/types/generated` 干净。

---

## 实施记录（已全部完成）

按 P0 → P3 优先级全部落地，并附一次由验证过程发现并根除的**预存在竞态缺陷**。

### 性能项（对应上文编号）

| 项 | 落地要点 |
|----|----------|
| P0-1 | `watcher.rs` 在整读前先比 change-id，未变化直接短睡跳过（四类 marker 写入点已核对，门控不影响标记不变量） |
| P0-2 | 新增 `src/lib/clipViewItems.ts`（WeakMap 稳定包装对象）；`format.ts` Intl 全部模块级单例；`ClipListPane` 虚拟滚动（固定高 windowing + flex spacer + rAF 节流）；搜索 120ms 防抖 + `loadInFlight` 复用 + 去掉 `slice(0, 120)` 截断 |
| P0-3 | `mocr_engine.rs` 解码循环改零拷贝 View 引用，消除每步 ~605KB 克隆（最多 299 步） |
| P0-4 | `Cargo.toml` 增加 `[profile.release]`：`lto = "thin"`、`codegen-units = 1`、`strip = "symbols"`（panic 保持 unwind：tokio `catch_unwind` 依赖） |
| P1-7 | `secrets.rs` 进程内读缓存（put_/delete_ 失效、失败不缓存）；legacy bigmodel 清理从设置读路径移入 `Store::migrate()` 一次执行；连接池**经实测否决**——单连接成本仅 ~30-50µs，真正痛点是 keyring 与下方竞态（另案修复） |
| P1-8 | `lib.rs` `DeviceLinkRegistry::start` 改由 `async_runtime::spawn` 后台执行，托盘/窗口/快捷键先行；就绪前命令层经 `DeviceRegistryExt` 优雅降级（设备面板不在启动路径，无可见错误） |
| P1-9 | 图片接收一次解码双消费者（落库 + 剪贴板共用位图）；`from_utf8_lossy` Cow 承接去掉整体 `to_string` |
| P2-10 | 四个计数合并为单次全表扫描（删两个计数函数）；`datetime(last_captured_at)` 表达式索引替换无查询使用的裸列索引（表重建路径同步） |
| P2-12 | Paddle 引擎缓存 `Box::leak` → `Arc`（`OcrEngine` 经 `unsafe impl Send+Sync` 满足静态缓存） |
| P2-13 | automation `drain_stream` 连接复用 + 16 行/100ms 批量落盘（前端仍逐行实时） |
| P2-14 | 读命令（list/search/snapshot）与云同步、连通性测试命令改 `async` + `spawn_blocking`（wry 的 IPC handler 在主线程，同步命令会冻结窗口） |
| P2-15 | 冻结帧写盘去整帧 `clone()`（单遍 RGBA→RGB）；会话固定显示器拓扑，提交时复用（防增删屏索引错位） |
| P2-17 | 重拨退避 ±25% 抖动 + 启动首拨 0–2s 随机延迟；心跳 `sleep` → `interval_at`（控制消息不再重置 30s 计时器） |
| P3 | openai.rs base64 移出循环；`sort_paddle_lines` 行聚合增量维护；ClipCard 缩略图 `loading="lazy" decoding="async"`；Tailwind `@source` 收窄；拖拽 ghost 直写 DOM；App.vue 三聚合合并单次遍历；设备列表事件合并窗口 |

### 附带修复：WAL 连接竞态（预存在的偶发写入失败）

验证阶段 `cargo test` 出现 4–6 个 LAN 集成测试随机失败（`attempt to write a readonly database`）。对照基线（stash 后同跑）确认**与本次优化无关，是预存在的 Store 缺陷**：

- **根因**：`Store::connect()` 每次操作开关独立连接。SQLite 在连接关闭路径会删除/截断 `-wal`/`-shm`；Windows 上该删除与并发连接的打开/重建互踩时，`winOpen` 对读写创建失败会**静默降级为只读**（`pWal->readOnly = WAL_RDONLY`），此后该连接的**所有写事务**恒报 `SQLITE_READONLY`（ext=8）。LAN 测试的链接轮询 + 会话写库即为现实触发形态；生产中表现为偶发丢失一次捕获落库。
- **复现**：新增回归测试 `store::tests::concurrent_connect_close_never_readonly`（4 写 + 4 读线程高频开关连接，修复前 ~2s 内必现，且与 iroh/LAN 无关）。
- **修复**（`store.rs`）：
  1. 每条连接设置 `SQLITE_FCNTL_PERSIST_WAL`（SQLite 官方为 Windows 提供的开关，禁止关闭时删除 `-wal`/`-shm`）——删除/重建竞态从根源消失；
  2. `Store::new` 锚连接一次性完成 WAL 切换，`connect()` 不再逐连接执行 `journal_mode` pragma（该 pragma 也触发 `-wal` 打开，是另一个竞态入口）。
- **验证**：回归测试 5 连跑全绿；LAN 模块串行 7/7、4 线程 7/7；全量 `cargo test` 两连跑全绿（290 lib + 2 bin）。

### 验证结果

- `npm run lint` 干净；`npm test` 290/290；`npm run build` 成功（vendor 99.66kB / index 345.50kB / 各窗口独立 chunk）。
- `cargo check --all-targets` 零警告；`cargo test` 全绿（含新增回归测试、搜索防抖、虚拟滚动、legacy 清理、jitter 边界等用例）。

### 已知遗留（未在本次范围）

- 多显示器截图的多帧驻留（P2-15 仅去掉克隆，未改为逐屏懒捕获）与 mocr 探测缓存（P3）留待后续按需处理。
- `Store` 连接池（复用单连接减少开关）在竞态修复后收益进一步降低，暂不引入；如未来出现高频写吞吐瓶颈再评估。
