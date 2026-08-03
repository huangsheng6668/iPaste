# 跨分类搜索（历史优先回退）设计

日期：2026-08-03
状态：设计待评审

## 1. 背景与目标

当前搜索行为绑定在「当前视图」上：在历史视图搜索只查 `clips` 表，在某个分类视图搜索只查该分类条目。用户希望：在历史视图搜索时，若历史无任何命中，自动回退到所有分类中搜索，按分类分组展示命中条目。

### 目标
- 在历史视图输入关键词时，先查历史；历史命中总数为 0 时，回退搜全部分类条目。
- 回退结果按分类分组展示，每条标注来源分类，点击可复制，不跳转到该分类。

### 非目标
- 不改变分类视图内的搜索行为（分类视图仍只搜当前分类，不回退）。
- 不改变置顶、分页加载更多在历史视图下的现有行为。
- 不引入云端跨设备搜索。

## 2. 关键决策（已与用户确认）

| 决策点 | 选择 |
|---|---|
| 搜索形态 | 优先级回退：历史为空才搜分类 |
| 分类降级范围 | 搜全部分类，按分类分组 |
| 结果交互 | 仍可复制；显示来源分类标签；不跳转 |
| 回退判定粒度 | 历史命中**总数 = 0** 才回退（P2） |
| 适用范围 | 仅历史视图；分类视图内搜索不回退 |

## 3. 方案

后端新增一个 Tauri 命令 `search_with_fallback`，封装「历史优先、空则回退分类」的两级逻辑。前端在历史视图的搜索流程改为调用该命令；分类视图维持现状。

**不改动** `list_clips` 及其现有调用方，保持单一职责与契约稳定。

### 数据流

```
前端搜索框（历史视图，selectedCategoryId === "history"）
  └─> search_with_fallback(query, offset, limit)
        1. total = COUNT(clips WHERE 匹配 query)
        2. if total > 0:
              return HistoryPage { clips, has_more, total_count, all_count }
        3. else (total == 0):
              groups = 查 category_items 匹配 query，按 category_id 分组
              return CategoryHits { groups: [{ category, items }] }
```

前端按返回类型渲染不同 UI。

## 4. 组件设计

### 4.1 Rust 后端

#### 4.1.1 数据类型（`models.rs`）

```rust
// 复用现有 ClipPage 作为 HistoryPage
pub(crate) struct CategoryHitGroup {
    pub(crate) category: Category,
    pub(crate) items: Vec<CategoryItem>,
}

// 带 tag 的 union，序列化为带 "kind" 字段的 JSON 便于前端判别
#[serde(tag = "kind")]
pub(crate) enum SearchResult {
    History { page: ClipPage },
    CategoryHits { groups: Vec<CategoryHitGroup> },
}
```

#### 4.1.2 Store 方法（`store.rs`）

新增：
- `fn search_with_fallback(&self, offset, limit, search) -> Result<SearchResult, String>`
  - 内部先用一个 `COUNT` 查询历史命中总数
  - `> 0`：复用 `list_clips_page_with_conn` 返回 `SearchResult::History`
  - `= 0`：调 `search_all_category_items_with_conn` 分组返回 `SearchResult::CategoryHits`

新增（私有）：
- `fn search_all_category_items_with_conn(&self, conn, search) -> Result<Vec<CategoryHitGroup>, String>`
  - SQL：`SELECT ... FROM category_items WHERE 匹配 query ORDER BY is_pinned DESC, sort_order ASC, datetime(created_at) DESC`
  - 匹配字段：`display_name / preview_text / text / clip_type`，与历史搜索字段保持一致；图片条目按 `'图片 image' LIKE ?` 处理，复用历史搜索的同款规则
  - 拉取后在 Rust 内按 `category_id` 分组，并 join 出对应 `Category` 元数据（名称、颜色）
  - 空查询字符串：返回空 `groups`（回退语义仅在「搜索词非空且历史为 0」时触发；空搜索本就不会让历史为 0，但防御性处理）

边界处理：`offset > 0` 但当前是回退结果时（即上一页是历史、本页历史已耗尽）——不在首版支持回退分页。首版约定：**回退结果只返回首页一次性结果，不分页**。理由：回退是「历史完全没有，给你分类里的相关条目」的兜底，量级通常小；分页会引入跨类型续页的复杂状态机，YAGNI。

#### 4.1.3 命令（`commands.rs`）

```rust
#[tauri::command]
pub(crate) fn search_with_fallback(
    state: tauri::State<'_, AppState>,
    offset: usize,
    limit: usize,
    search: String,
) -> Result<SearchResult, String> {
    state.store.search_with_fallback(offset, limit, search)
}
```

在 `lib.rs` 的 `invoke_handler!` 注册 `search_with_fallback`。

### 4.2 前端

#### 4.2.1 API 封装（`src/lib/ipasteApi.ts`）

新增 `searchWithFallback(offset, limit, search): Promise<SearchResult>`，对应 `invoke("search_with_fallback", ...)`。
保留现有 `listClips`（其他路径仍可能用）。

#### 4.2.2 类型（`src/types.ts`）

```ts
export type SearchResult =
  | { kind: "History"; page: ClipPage }
  | { kind: "CategoryHits"; groups: Array<{ category: Category; items: CategoryItem[] }> };
```

#### 4.2.3 Store（`src/stores/ipasteStore.ts`）

- 新增响应式状态 `fallbackGroups`（仅当当前结果为回退结果时填充）。
- 修改 `reloadClips` / `loadMoreClips`：当 `selectedCategoryId.value === "history"` 且 `search.value` 非空时，调用 `searchWithFallback` 而非 `listClips`。
  - 收到 `History`：与现在一样填充 `clips` / `hasMoreClips` / 计数。
  - 收到 `CategoryHits`：清空 `clips`，填充 `fallbackGroups`，`hasMoreClips = false`。
- `loadMoreClips`：若当前是回退结果，直接 return（不支持分页）。
- 清空搜索词时：清空 `fallbackGroups`，恢复正常历史列表。
- 在分类视图（`selectedCategoryId !== "history"`）下，搜索流程完全不经过新命令，行为不变。

#### 4.2.4 视图渲染（`src/App.vue`）

- 历史视图主体列表区：当 `fallbackGroups` 非空时，渲染分组卡片（每组显示分类名/颜色 + 该组条目列表），否则渲染原历史 `clips` 列表。
- 分组内的 `ClipCard`：复用现有卡片组件，传入 `CategoryItem`；新增「来自：{分类名}」来源标签显示。
- 点击复制：复用分类条目现有的复制路径（即 `collection === "category"` 的复制逻辑），不触发跳转。

## 5. 错误处理

- 后端 SQL 错误统一 `map_err(|e| e.to_string())` 返回 `Err(String)`，与现有命令风格一致。
- 前端 try/catch 写入 `error.value`，与 `reloadClips` 现有错误处理一致。
- 竞态：复用 `clipRequestId` 模式，丢弃过期请求结果。

## 6. 测试策略

### 后端单元测试（`store.rs` 既有测试模块风格）
- 历史**有**命中 → 返回 `SearchResult::History`，内容与 `list_clips` 一致。
- 历史**无**命中、某分类有条目匹配 → 返回 `CategoryHits`，分组正确，来源分类元数据正确。
- 历史**无**命中、无任何分类条目匹配 → 返回 `CategoryHits { groups: [] }`。
- 空搜索词 → 返回 `History`（不应触发回退，因空词历史必然非空；防御性断言）。
- 图片条目的 `'图片 image' LIKE ?` 规则在分类回退中同样生效。

### 前端
- 手动验证：历史视图输入「无历史命中但分类有」的词 → 看到分组卡片；清空 → 恢复历史列表。
- 分类视图搜索行为不变（回归）。

## 7. 影响范围与风险

- **改动文件**：`models.rs`、`store.rs`、`commands.rs`、`lib.rs`、`src/lib/ipasteApi.ts`、`src/types.ts`、`src/stores/ipasteStore.ts`、`src/App.vue`（+ 可能的 `ClipCard` 来源标签）。
- **不破坏**：`list_clips` / `clear_clips` / `prune_expired` / 分类 CRUD / 云同步契约均不变。
- **风险点**：回退结果不分页——若分类命中极多（数百条），首屏一次性渲染可能卡顿。首版接受（剪贴板场景单分类条目通常有限）；若实测有问题再加分页。
- **国际化**：来源标签文案需加入 `i18n.ts`（如 `search.fromCategory: "来自 {{name}}"` 及各语言）。
