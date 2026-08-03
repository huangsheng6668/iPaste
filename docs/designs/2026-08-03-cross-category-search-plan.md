# 跨分类回退搜索 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在历史视图搜索时，若历史无命中，自动回退搜全部分类条目并按分类分组展示。

**Architecture:** 后端新增 Tauri 命令 `search_with_fallback`，封装「历史优先、空则回退分类」两级逻辑，返回带 `kind` 判别的 union 类型。前端在历史视图搜索流程改调该命令，按返回类型渲染历史列表或分组卡片。不改动 `list_clips` 契约。

**Tech Stack:** Rust + rusqlite + Tauri 2；Vue 3 + Pinia（composition API store）+ TypeScript。

## Global Constraints

- Rust 结构体凡经 Tauri IPC 传输，统一 `#[serde(rename_all = "camelCase")]`（既有约定，见 `models.rs`）。前端 TS 类型字段均为 camelCase。
- 后端错误统一 `map_err(|e| e.to_string())` 返回 `Result<T, String>`，与现有命令风格一致。
- 搜索匹配规则必须与历史搜索 `list_clips_page_with_conn`（`store.rs:769`）一致：`display_name / preview_text / clip_type / text` 四字段，图片条目走 `(clip_type = 'image' AND '图片 image' LIKE ?)` 分支，文本条目走 `(clip_type != 'image' AND lower(text) LIKE ?)`。
- 回退结果**不分页**，一次性返回。
- 仅历史视图（`selectedCategoryId === "history"` 且 `search` 非空）触发回退；分类视图搜索行为不变。
- 文档与计划存 `docs/designs/` 与 `docs/superpowers/`（后者被 .gitignore，本地管理不入库）。本计划本身的提交规则：本地 commit、不 push。

---

## File Structure

| 文件 | 责任 | 动作 |
|---|---|---|
| `src-tauri/src/models.rs` | 新增 `CategoryHitGroup`、`SearchResult` 类型 | 新增 |
| `src-tauri/src/store.rs` | 新增 `search_with_fallback`、私有 `search_all_category_items_with_conn`、私有 `count_clips_matching_with_conn` | 新增方法 |
| `src-tauri/src/store.rs`（测试模块） | 新增 `#[cfg(test)]` 内存 SQLite 测试基建 + 搜索用例 | 新增 |
| `src-tauri/src/commands.rs` | 新增 `search_with_fallback` 命令 | 新增 |
| `src-tauri/src/lib.rs` | `invoke_handler!` 注册 `search_with_fallback` | 修改 |
| `src/types.ts` | 新增 `SearchResult` union 类型 | 新增 |
| `src/lib/ipasteApi.ts` | 新增 `searchWithFallback` 封装 + mock fallback | 修改 |
| `src/stores/ipasteStore.ts` | `fallbackGroups` 状态；改造 `reloadClips`/`loadMoreClips` 走回退命令 | 修改 |
| `src/App.vue` | 历史视图渲染分组卡片；来源标签 | 修改 |
| `src/i18n.ts` | 新增 `search.fromCategory` 各语言文案 | 修改 |

---

## Task 1: 后端测试基建（内存 SQLite + 临时 Store）

为后续 TDD 任务提供可在内存/临时目录跑的 Store。当前 `src-tauri/src/store.rs` 无任何测试。

**Files:**
- Modify: `src-tauri/src/store.rs`（末尾新增 `#[cfg(test)]` 模块）

**Interfaces:**
- Produces: `Store::open_in_memory()` 测试辅助（仅 `#[cfg(test)]`），返回一个用临时目录作 `image_dir` 的 `Store`，便于插入 clips / categories / category_items 后断言。

- [ ] **Step 1: 读 `Store::new` 签名与字段，确认构造方式**

Run: 用 Read 看 `src-tauri/src/store.rs` 顶部 `struct Store` 与 `impl Store { fn new` / `fn connect`。
确认 `db_path: PathBuf` 字段如何被初始化，以便测试用 `tempfile` 或 `std::env::temp_dir` 构造。

- [ ] **Step 2: 加测试辅助方法 + 一个 trivial 测试，跑通 cargo test**

在 `store.rs` 末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> Store {
        let dir = std::env::temp_dir().join(format!(
            "ipaste-test-{}-{}",
            std::process::id(),
            uuid_like()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");
        Store::new(db_path).expect("store init")
    }

    // 简易唯一串，避免引入 uuid 依赖；若项目已有 new_id() 可直接复用
    fn uuid_like() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{nanos}")
    }

    #[test]
    fn store_initializes_empty() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM clips", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
```

若 `Store::new` 名字不同（如 `open`），按实际改。若 `connect` 是私有且返回 `Connection`，保持一致。

- [ ] **Step 3: 运行测试，确认通过**

Run: `cd src-tauri && cargo test store_initializes_empty -- --nocapture`
Expected: PASS。若 `Store::new` 不存在或签名不同，按 Step 1 读到的实际 API 调整辅助函数后再跑。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/store.rs
git commit -m "test: add in-memory store test harness"
```

---

## Task 2: 新增 SearchResult / CategoryHitGroup 类型

**Files:**
- Modify: `src-tauri/src/models.rs`（在 `ClipPage` 定义之后追加）

**Interfaces:**
- Produces:
  - `CategoryHitGroup { category: Category, items: Vec<CategoryItem> }`
  - `enum SearchResult { History { page: ClipPage }, CategoryHits { groups: Vec<CategoryHitGroup> } }`
  - 序列化为 `{"kind":"history","page":{...}}` 或 `{"kind":"categoryHits","groups":[{"category":{...},"items":[...]}]}`（注意 serde 对 enum 默认 externally tagged；这里显式用 `#[serde(tag = "kind", rename_all = "camelCase")]`）。

- [ ] **Step 1: 写编译期断言测试（确保类型存在且可序列化）**

在 `src-tauri/src/models.rs` 末尾新增：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_result_history_serializes_with_kind_tag() {
        let page = ClipPage {
            clips: vec![],
            has_more: false,
            total_count: 0,
            all_count: 0,
        };
        let json = serde_json::to_string(&SearchResult::History { page }).unwrap();
        assert!(json.contains(r#""kind":"history""#), "got: {json}");
    }

    #[test]
    fn search_result_category_hits_serializes_with_kind_tag() {
        let res = SearchResult::CategoryHits { groups: vec![] };
        let json = serde_json::to_string(&res).unwrap();
        assert!(json.contains(r#""kind":"categoryHits""#), "got: {json}");
    }
}
```

若 `serde_json` 不在 dev-dependencies，在 `src-tauri/Cargo.toml` 的 `[dev-dependencies]` 加 `serde_json`（若已是依赖则跳过）。

- [ ] **Step 2: 运行测试，确认失败（类型未定义）**

Run: `cd src-tauri && cargo test --lib search_result_`
Expected: 编译失败，`cannot find type SearchResult`。

- [ ] **Step 3: 实现类型**

在 `models.rs` 的 `ClipPage` 定义之后追加：

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CategoryHitGroup {
    pub(crate) category: Category,
    pub(crate) items: Vec<CategoryItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum SearchResult {
    History { page: ClipPage },
    CategoryHits { groups: Vec<CategoryHitGroup> },
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cd src-tauri && cargo test --lib search_result_`
Expected: 两个测试 PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/models.rs src-tauri/Cargo.toml
git commit -m "feat(models): add SearchResult/CategoryHitGroup types"
```

---

## Task 3: Store —— 历史命中计数辅助

为回退判定提供「历史命中总数」查询，复用历史搜索的匹配规则。

**Files:**
- Modify: `src-tauri/src/store.rs`

**Interfaces:**
- Produces: `fn count_clips_matching_with_conn(&self, conn: &Connection, search: &str) -> Result<usize, String>`，返回历史表命中 `search` 的行数。空 `search` 返回 `clips` 总行数（与 `list_clips_page_with_conn` 的 `?1 = ''` 短路语义一致）。

- [ ] **Step 1: 写失败测试**

在 Task 1 的 `mod tests` 内追加：

```rust
#[test]
fn count_clips_matching_respects_query() {
    let store = temp_store();
    let conn = store.connect().unwrap();
    seed_clip(&conn, "text", "hello world", "hello world");
    seed_clip(&conn, "text", "rust lang", "rust lang");
    assert_eq!(store.count_clips_matching_with_conn(&conn, "hello").unwrap(), 1);
    assert_eq!(store.count_clips_matching_with_conn(&conn, "").unwrap(), 2);
    assert_eq!(store.count_clips_matching_with_conn(&conn, "nomatch").unwrap(), 0);
}

fn seed_clip(conn: &rusqlite::Connection, clip_type: &str, preview: &str, text: &str) {
    conn.execute(
        "INSERT INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
         VALUES (?1, ?2, ?3, NULL, ?4, ?5, 'test', ?6, 0, 0)",
        rusqlite::params![
            crate::lib::new_id(),
            clip_type,
            crate::lib::hash_text(text),
            preview,
            text,
            chrono::Utc::now().to_rfc3339(),
        ],
    )
    .unwrap();
}
```

若 `new_id` / `hash_text` 不在 `lib.rs` 导出，改用测试内的本地版（直接用 `format!("id-{}", counter)` 与简单 hash）。先 Read `lib.rs` 确认可见性。

- [ ] **Step 2: 运行测试，确认失败**

Run: `cd src-tauri && cargo test count_clips_matching_respects_query`
Expected: 编译失败，方法未定义。

- [ ] **Step 3: 实现方法**

在 `store.rs` 的 `impl Store` 内（`list_clips_page_with_conn` 附近）追加：

```rust
fn count_clips_matching_with_conn(
    &self,
    conn: &Connection,
    search: &str,
) -> Result<usize, String> {
    let query = search.trim().to_lowercase();
    let pattern = format!("%{query}%");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM clips
             WHERE ?1 = ''
                OR lower(COALESCE(display_name, '')) LIKE ?2
                OR lower(preview_text) LIKE ?2
                OR lower(clip_type) LIKE ?2
                OR (clip_type != 'image' AND lower(text) LIKE ?2)
                OR (clip_type = 'image' AND '图片 image' LIKE ?2)",
            params![query.as_str(), pattern.as_str()],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    Ok(count as usize)
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cd src-tauri && cargo test count_clips_matching_respects_query`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/store.rs
git commit -m "feat(store): add count_clips_matching_with_conn"
```

---

## Task 4: Store —— 跨分类搜索 + 分组

**Files:**
- Modify: `src-tauri/src/store.rs`

**Interfaces:**
- Produces:
  - `fn search_all_category_items_with_conn(&self, conn: &Connection, search: &str) -> Result<Vec<CategoryHitGroup>, String>`：查所有匹配 `search` 的 `category_items`，按 `category_id` 分组，每组附 `Category` 元数据；空 search 返回空 vec。
  - 分组顺序：按分类 `sort_order` 升序；组内条目按 `is_pinned DESC, sort_order ASC, datetime(created_at) DESC`。

- [ ] **Step 1: 写失败测试**

在 `mod tests` 追加：

```rust
#[test]
fn search_all_category_items_groups_by_category() {
    let store = temp_store();
    let conn = store.connect().unwrap();
    let cat_a = create_category(&conn, "A", "#f00", 0);
    let cat_b = create_category(&conn, "B", "#0f0", 1);
    seed_category_item(&conn, &cat_a, "text", "alpha token", "alpha token");
    seed_category_item(&conn, &cat_a, "text", "beta token", "beta token");
    seed_category_item(&conn, &cat_b, "text", "alpha other", "alpha other");

    let groups = store.search_all_category_items_with_conn(&conn, "alpha").unwrap();
    assert_eq!(groups.len(), 2, "two categories have alpha hits");
    assert_eq!(groups[0].category.name, "A", "lower sort_order first");
    assert_eq!(groups[0].items.len(), 1);
    assert_eq!(groups[1].category.name, "B");
    assert_eq!(groups[1].items.len(), 1);
}

#[test]
fn search_all_category_items_empty_query_returns_empty() {
    let store = temp_store();
    let conn = store.connect().unwrap();
    let cat = create_category(&conn, "A", "#f00", 0);
    seed_category_item(&conn, &cat, "text", "x", "x");
    let groups = store.search_all_category_items_with_conn(&conn, "").unwrap();
    assert!(groups.is_empty());
}

fn create_category(conn: &rusqlite::Connection, name: &str, color: &str, sort_order: i64) -> String {
    let id = crate::lib::new_id();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO categories (id, name, color, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        rusqlite::params![id, name, color, sort_order, now],
    )
    .unwrap();
    id
}

fn seed_category_item(conn: &rusqlite::Connection, category_id: &str, clip_type: &str, preview: &str, text: &str) {
    let id = crate::lib::new_id();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO category_items (id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, 0, ?8, ?8, 'local', 0)",
        rusqlite::params![id, category_id, id, clip_type, crate::lib::hash_text(text), preview, text, now],
    )
    .unwrap();
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cd src-tauri && cargo test search_all_category_items_`
Expected: 编译失败，方法未定义。

- [ ] **Step 3: 实现**

在 `impl Store` 内追加：

```rust
fn search_all_category_items_with_conn(
    &self,
    conn: &Connection,
    search: &str,
) -> Result<Vec<CategoryHitGroup>, String> {
    let query = search.trim().to_lowercase();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let pattern = format!("%{query}%");
    let mut stmt = conn
        .prepare(
            "SELECT id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned
             FROM category_items
             WHERE lower(COALESCE(display_name, '')) LIKE ?1
                OR lower(preview_text) LIKE ?1
                OR lower(clip_type) LIKE ?1
                OR (clip_type != 'image' AND lower(text) LIKE ?1)
                OR (clip_type = 'image' AND '图片 image' LIKE ?1)
             ORDER BY is_pinned DESC, sort_order ASC, datetime(created_at) DESC",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(params![pattern.as_str()], map_category_item)
        .map_err(|error| error.to_string())?;
    let items: Vec<CategoryItem> = collect_rows(rows)?;

    let categories = self.list_categories_with_conn(conn)?;
    let category_by_id: std::collections::HashMap<String, Category> =
        categories.into_iter().map(|c| (c.id.clone(), c)).collect();

    // 保持分组顺序：按分类 sort_order（list_categories_with_conn 已排序）
    // 用 IndexMap 以保留首次出现顺序；避免引入新依赖则手动维护顺序
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<CategoryItem>> =
        std::collections::HashMap::new();
    for item in items {
        let cat_id = item.category_id.clone();
        if !groups.contains_key(&cat_id) {
            order.push(cat_id.clone());
            groups.insert(cat_id.clone(), Vec::new());
        }
        groups.get_mut(&cat_id).unwrap().push(item);
    }

    let result: Vec<CategoryHitGroup> = order
        .into_iter()
        .filter_map(|cat_id| {
            category_by_id.get(&cat_id).map(|category| CategoryHitGroup {
                category: category.clone(),
                items: groups.remove(&cat_id).unwrap_or_default(),
            })
        })
        .collect();
    Ok(result)
}
```

注：`order` 仅在插入时记录顺序，但 `items` 已按 `(is_pinned, sort_order, created_at)` 排序，**不保证按分类 sort_order 顺序首次出现**。为严格满足「组按分类 sort_order 升序」，改为先按 `list_categories_with_conn` 的顺序遍历分类、再从 `groups` 取条目：

把上面 `result` 构造替换为：

```rust
let ordered_categories = self.list_categories_with_conn(conn)?;
let result: Vec<CategoryHitGroup> = ordered_categories
    .into_iter()
    .filter_map(|category| {
        groups.remove(&category.id).map(|items| CategoryHitGroup { category, items })
    })
    .collect();
Ok(result)
```

并删除前面的 `let mut order` 及其相关插入逻辑（`groups` 仍按 `item.category_id` 收集即可）。同时上面已调用过一次 `list_categories_with_conn`，合并为只调一次：提前拿 `ordered_categories`，构造 `category_by_id` 与最终遍历都用它。

- [ ] **Step 4: 运行测试，确认通过**

Run: `cd src-tauri && cargo test search_all_category_items_`
Expected: 两个测试 PASS。若 `groups[0].category.name` 不是 "A"，说明分组顺序未按 sort_order —— 回到 Step 3 检查最终用 `ordered_categories` 遍历的逻辑。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/store.rs
git commit -m "feat(store): add cross-category item search with grouping"
```

---

## Task 5: Store —— search_with_fallback 编排

**Files:**
- Modify: `src-tauri/src/store.rs`

**Interfaces:**
- Produces: `pub(crate) fn search_with_fallback(&self, offset: usize, limit: usize, search: &str) -> Result<SearchResult, String>`
  - 历史 `count > 0` → `SearchResult::History { page: list_clips_page_with_conn(...) }`
  - 历史 `count == 0` → `SearchResult::CategoryHits { groups: search_all_category_items_with_conn(...) }`

- [ ] **Step 1: 写失败测试**

在 `mod tests` 追加：

```rust
#[test]
fn fallback_returns_history_when_history_has_hits() {
    let store = temp_store();
    let conn = store.connect().unwrap();
    seed_clip(&conn, "text", "hello", "hello");
    let res = store.search_with_fallback(0, 20, "hello").unwrap();
    match res {
        SearchResult::History { page } => assert_eq!(page.clips.len(), 1),
        other => panic!("expected History, got {:?}", other),
    }
}

#[test]
fn fallback_returns_category_hits_when_history_empty() {
    let store = temp_store();
    let conn = store.connect().unwrap();
    let cat = create_category(&conn, "A", "#f00", 0);
    seed_category_item(&conn, &cat, "text", "secret token", "secret token");
    let res = store.search_with_fallback(0, 20, "secret").unwrap();
    match res {
        SearchResult::CategoryHits { groups } => {
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0].items.len(), 1);
        }
        other => panic!("expected CategoryHits, got {:?}", other),
    }
}

#[test]
fn fallback_returns_empty_category_hits_when_nowhere_matches() {
    let store = temp_store();
    let _conn = store.connect();
    let res = store.search_with_fallback(0, 20, "ghost").unwrap();
    match res {
        SearchResult::CategoryHits { groups } => assert!(groups.is_empty()),
        other => panic!("expected CategoryHits, got {:?}", other),
    }
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cd src-tauri && cargo test fallback_`
Expected: 编译失败，方法未定义。

- [ ] **Step 3: 实现**

在 `impl Store` 内追加：

```rust
pub(crate) fn search_with_fallback(
    &self,
    offset: usize,
    limit: usize,
    search: &str,
) -> Result<SearchResult, String> {
    let conn = self.connect()?;
    let total = self.count_clips_matching_with_conn(&conn, search)?;
    if total > 0 {
        let page = self.list_clips_page_with_conn(&conn, offset, limit, search)?;
        Ok(SearchResult::History { page })
    } else {
        let groups = self.search_all_category_items_with_conn(&conn, search)?;
        Ok(SearchResult::CategoryHits { groups })
    }
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cd src-tauri && cargo test fallback_`
Expected: 三个测试 PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/store.rs
git commit -m "feat(store): add search_with_fallback orchestration"
```

---

## Task 6: 暴露 Tauri 命令 + 注册

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: Tauri 命令 `search_with_fallback(state, offset, limit, search) -> Result<SearchResult, String>`，前端 `invoke("search_with_fallback", { offset, limit, search })` 可调。

- [ ] **Step 1: 在 `commands.rs` 加命令**

在 `list_clips` 命令附近追加：

```rust
#[tauri::command]
pub(crate) fn search_with_fallback(
    state: tauri::State<'_, AppState>,
    offset: usize,
    limit: usize,
    search: String,
) -> Result<SearchResult, String> {
    state.store.search_with_fallback(offset, limit, &search)
}
```

确认 `SearchResult` 已通过 `use crate::models::*;` 或显式导入可见（参考 `commands.rs` 顶部既有 import）。

- [ ] **Step 2: 在 `lib.rs` 的 `invoke_handler!` 注册**

在 `src-tauri/src/lib.rs:130` 附近（`list_clips,` 同区）追加一行 `search_with_fallback,`。

- [ ] **Step 3: 编译确认**

Run: `cd src-tauri && cargo check`
Expected: 编译通过、无警告（除既有 CRLF 等无关警告）。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(commands): expose search_with_fallback to frontend"
```

---

## Task 7: 前端类型 + API 封装

**Files:**
- Modify: `src/types.ts`
- Modify: `src/lib/ipasteApi.ts`

**Interfaces:**
- Produces:
  - `SearchResult` union（`src/types.ts`）
  - `ipasteApi.searchWithFallback(offset, limit, search)`（`src/lib/ipasteApi.ts`）

- [ ] **Step 1: 加 TS 类型**

在 `src/types.ts` 末尾追加：

```ts
export type CategoryHitGroup = {
  category: Category;
  items: CategoryItem[];
};

export type SearchResult =
  | { kind: "history"; page: ClipPage }
  | { kind: "categoryHits"; groups: CategoryHitGroup[] };
```

确认 `ClipPage` 已在 `types.ts` 导出；若未导出，从现有用法（如 store 里的 `page.clips`）补一个类型或直接内联字段。

- [ ] **Step 2: 加 API 封装 + mock fallback**

在 `src/lib/ipasteApi.ts` 的 `ipasteApi` 对象内（`listClips` 之后）追加：

```ts
searchWithFallback(offset = 0, limit = 20, search = "") {
  const query = search.trim().toLowerCase();
  const matchedClips = query
    ? mockClips.filter((item) =>
        [item.displayName ?? "", item.previewText, item.clipType, item.clipType === "image" ? "image" : item.text]
          .some((value) => value.toLowerCase().includes(query)),
      )
    : mockClips;
  if (matchedClips.length > 0) {
    return call<SearchResult>("search_with_fallback", { offset, limit, search }, {
      kind: "history",
      page: {
        clips: matchedClips.slice(offset, offset + limit),
        hasMore: offset + limit < matchedClips.length,
        totalCount: matchedClips.length,
        allCount: mockClips.length,
      },
    });
  }
  const groups: CategoryHitGroup[] = [];
  for (const cat of mockCategories) {
    const items = mockCategoryItems.filter(
      (item) =>
        item.categoryId === cat.id &&
        [item.displayName ?? "", item.previewText, item.clipType, item.clipType === "image" ? "image" : item.text]
          .some((value) => value.toLowerCase().includes(query)),
    );
    if (items.length > 0) groups.push({ category: cat, items });
  }
  return call<SearchResult>("search_with_fallback", { offset, limit, search }, { kind: "categoryHits", groups });
},
```

在文件顶部 import 中加入 `SearchResult, CategoryHitGroup`（按既有 import 风格）。

- [ ] **Step 3: 类型检查**

Run: `npm run type-check`（或项目既有脚本，如 `vue-tsc`）
Expected: 无类型错误。若 `ClipPage` 类型缺失，补上。

- [ ] **Step 4: 提交**

```bash
git add src/types.ts src/lib/ipasteApi.ts
git commit -m "feat(api): add searchWithFallback with mock fallback"
```

---

## Task 8: Store —— 接入回退搜索 + fallbackGroups 状态

**Files:**
- Modify: `src/stores/ipasteStore.ts`

**Interfaces:**
- Consumes: `ipasteApi.searchWithFallback`（Task 7）、`SearchResult`（Task 7）
- Produces: 响应式 `fallbackGroups: Ref<CategoryHitGroup[]>`；改造 `reloadClips` / `loadMoreClips`。

- [ ] **Step 1: 加状态与导入**

在 `src/stores/ipasteStore.ts` 状态声明区（`clips` / `hasMoreClips` 附近）加：

```ts
const fallbackGroups = ref<CategoryHitGroup[]>([]);
```

并在文件顶部 import 中加入 `CategoryHitGroup`（来自 `src/types.ts`）。

- [ ] **Step 2: 改造 reloadClips（仅历史视图 + 非空搜索走回退命令）**

把现有 `reloadClips`（`ipasteStore.ts:202`）的实现替换为：

```ts
async function reloadClips() {
  const requestId = ++clipRequestId;
  try {
    const isHistorySearch = selectedCategoryId.value === "history" && search.value.trim() !== "";
    const result = isHistorySearch
      ? await ipasteApi.searchWithFallback(0, CLIP_PAGE_SIZE, search.value)
      : null;
    const page = result ? null : await ipasteApi.listClips(0, CLIP_PAGE_SIZE, search.value);
    if (requestId !== clipRequestId) return;

    if (result?.kind === "history") {
      clips.value = result.page.clips;
      hasMoreClips.value = result.page.hasMore;
      visibleHistoryTotalCount.value = result.page.totalCount;
      clipTotalCount.value = result.page.allCount;
      fallbackGroups.value = [];
    } else if (result?.kind === "categoryHits") {
      clips.value = [];
      hasMoreClips.value = false;
      visibleHistoryTotalCount.value = 0;
      clipTotalCount.value = 0;
      fallbackGroups.value = result.groups;
    } else if (page) {
      clips.value = page.clips;
      hasMoreClips.value = page.hasMore;
      visibleHistoryTotalCount.value = page.totalCount;
      clipTotalCount.value = page.allCount;
      fallbackGroups.value = [];
    }
    selectedIndex.value = 0;
  } catch (unknownError) {
    if (requestId === clipRequestId) {
      error.value = String(unknownError);
    }
  }
}
```

- [ ] **Step 3: 改造 loadMoreClips（回退结果不分页）**

把现有 `loadMoreClips`（`ipasteStore.ts:180`）的早返回条件扩充：在函数开头加 `if (fallbackGroups.value.length > 0) return;`。其余逻辑（调 `listClips` 续页）保持不变 —— 续页只在历史命中模式下触发，回退模式直接 return。

- [ ] **Step 4: 清空搜索词时清空 fallbackGroups**

找到 `search` ref 的重置点（搜索框清空、切换分类时）。在 `search.value = ""` 的位置之后补 `fallbackGroups.value = [];`。若切换分类（`selectCategory` 等）会重新拉数据，确保 `fallbackGroups` 在切到分类视图时也被清空（避免分类视图残留分组卡片）。

- [ ] **Step 5: 在 store return 中导出**

在 `ipasteStore.ts` 末尾的 `return` 对象中加入 `fallbackGroups`。

- [ ] **Step 6: 类型检查**

Run: `npm run type-check`
Expected: 无类型错误。

- [ ] **Step 7: 提交**

```bash
git add src/stores/ipasteStore.ts
git commit -m "feat(store): route history search through fallback command"
```

---

## Task 9: 视图渲染分组卡片 + 来源标签

**Files:**
- Modify: `src/App.vue`
- Modify: `src/i18n.ts`

**Interfaces:**
- Consumes: store 的 `fallbackGroups`、`CategoryHitGroup`、`CategoryItem`、分类复制路径。

- [ ] **Step 1: 加 i18n key**

在 `src/i18n.ts` 每种语言的 `search` 段（若不存在则在合适位置）加 key。先确认 `search` 段是否存在；若不存在，加到与 `category.*` 同级。中文示例：

```ts
"search.fromCategory": "来自 {{name}}",
```

英文：`"search.fromCategory": "From {{name}}"`。其余语言（日/韩/西/法/德）按各语言自然翻译，参数占位符保持 `{{name}}`。

- [ ] **Step 2: 在 App.vue 历史视图主体区渲染分组**

找到历史视图渲染 `clips` 列表的 `<template>` 区块。在该区块外层加条件：

```vue
<template v-if="store.fallbackGroups.length > 0">
  <div v-for="group in store.fallbackGroups" :key="group.category.id" class="fallback-group">
    <div class="fallback-group-header">
      <span
        class="fallback-group-dot"
        :style="{ backgroundColor: group.category.color }"
      />
      <span>{{ group.category.name }}</span>
      <span class="fallback-group-count">{{ group.items.length }}</span>
    </div>
    <ClipCard
      v-for="item in group.items"
      :key="item.id"
      :item="toCategoryClipViewItem(item, group.category.name)"
      @copy="..."
    />
  </div>
</template>
<template v-else>
  <!-- 原有 clips 列表渲染 -->
</template>
```

注意：
- `ClipCard` 的 props 必须按其既有契约传入。先 Read `src/components/ClipCard.vue` 确认 `item` 的形状（`ClipViewItem`），补一个 `toCategoryClipViewItem(item, categoryName)` 把 `CategoryItem` 映射成卡片期望的视图模型，并把来源标签文案塞进卡片可显示的字段（或卡片已有的 tag 机制）。
- 复制事件：复用现有 `collection === "category"` 的复制逻辑（即分类条目复制路径），不触发 `selectCategory` 跳转。

- [ ] **Step 3: 来源标签**

若 `ClipCard` 已支持展示 tag/label（参考历史条目上的 `api_key` 绿色标签），把来源分类名作为 tag 传入。若不支持，在卡片外层加一个 `<span class="source-tag">{{ t("search.fromCategory", { name: group.category.name }) }}</span>`。

- [ ] **Step 4: 样式（最小）**

在 `src/styles/main.css` 加最小样式（分组间距、来源标签颜色），保持与现有设计语言一致；不引入新颜色变量。

- [ ] **Step 5: 手动验证**

Run: `npm run tauri dev`
验证清单：
1. 历史视图输入「历史有命中」的词 → 正常历史列表，分页加载更多可用。
2. 历史视图输入「历史无命中、某分类有命中」的词 → 分组卡片，每组带来源标签，点击复制成功，**不跳转**到该分类。
3. 历史视图输入「哪都没有」的词 → 显示空状态（与现有空状态一致）。
4. 清空搜索词 → 恢复完整历史列表，无残留分组卡片。
5. 切到某分类视图再搜索 → 行为不变（只搜当前分类，不回退）。

- [ ] **Step 6: 提交**

```bash
git add src/App.vue src/i18n.ts src/styles/main.css
git commit -m "feat(ui): render fallback category groups with source label"
```

---

## Task 10: 整体回归 + 收尾

- [ ] **Step 1: 跑全部后端测试**

Run: `cd src-tauri && cargo test`
Expected: 全绿（含 Task 1-5 的新增用例）。

- [ ] **Step 2: 前端类型检查 + lint**

Run: `npm run type-check && npm run lint`（按项目既有脚本）
Expected: 无错误。

- [ ] **Step 3: 端到端冒烟**

Run: `npm run tauri dev`
走一遍 Task 9 Step 5 的验证清单，外加：
- 置顶历史条目在搜索时仍按既有规则出现。
- 清空历史记录后，分类条目仍可被回退搜索命中（验证 clips 表清空不影响 category_items 搜索）。

- [ ] **Step 4: 文档收尾**

在 `docs/designs/2026-08-03-cross-category-search-design.md` 末尾加一行实现状态说明（可选），本地 commit。

- [ ] **Step 5: 确认未推送**

Run: `git log origin/main..HEAD --oneline`
Expected: 列出本计划产生的所有本地 commit，**不执行 `git push`**。
