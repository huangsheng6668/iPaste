// store/clips.rs — 剪贴板捕获/查询/CRUD
use std::fs;
use std::path::PathBuf;

use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use super::Store;
use crate::models::{CapturedClipboardItem, ClipItem, ClipPage, ClipUpdate, SearchResult};
use crate::{
    clipboard::image_bytes_from_data_url,
    store::rows::{collect_rows, map_clip},
    util::{clean_clip_type_filter, clean_display_name, hash_text, new_id, now, preview, safe_filename},
    IMAGE_DIR,
};

/// 列表/检查器缩略图的最大边长（px）。卡片图标与检查器预览的渲染尺寸
/// 都在此范围内；4K 截图缩到 480px 后解码与纹理开销约为原来的 1/70。
const IMAGE_THUMBNAIL_MAX: u32 = 480;

/// ≥3 字符的搜索词构造成 FTS5 短语（trigram 最短命中长度 3）：
/// 引号包裹 + 内部双引号翻倍，让整个查询串做大小写不敏感子串匹配，
/// 与 LIKE 检索语义一致，且不被解释成 FTS 查询语法。更短的词（中文两字
/// 词很常见）返回 None，调用方回退 LIKE 扫描。
fn fts_match_phrase(query: &str) -> Option<String> {
    if query.chars().count() < 3 {
        return None;
    }
    // 引号包裹成短语；内部双引号翻倍转义（原始串字面量含两个双引号）。
    let escaped = query.replace('"', r#""""#);
    Some(format!(r#""{escaped}""#))
}

impl Store {
    pub(crate) fn insert_captured_item(
        &self,
        item: CapturedClipboardItem,
    ) -> Result<Option<(ClipItem, usize, bool)>, String> {
        let mut conn = self.connect()?;
        let existing: Option<ClipItem> = conn
            .query_row(
                "SELECT id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned
                 FROM clips WHERE content_hash = ?1",
                params![item.content_hash],
                map_clip,
            )
            .optional()
            .map_err(|error| error.to_string())?;

        if let Some(clip) = existing {
            // 去重命中路径单事务收口：原先 UPDATE（+可选 UPDATE）+ 回读 + COUNT
            // 各自 autocommit，每次捕获付 3-4 次独立提交。
            let tx = conn.transaction().map_err(|error| error.to_string())?;
            let captured_at = now();
            tx.execute(
                "UPDATE clips SET last_captured_at = ?1 WHERE id = ?2",
                params![captured_at, clip.id],
            )
            .map_err(|error| error.to_string())?;
            // LAN 接收的重命名仅在本地还没有重命名时补齐，不覆盖用户自己的命名。
            if item.display_name.is_some() && clip.display_name.is_none() {
                tx.execute(
                    "UPDATE clips SET display_name = ?1 WHERE id = ?2",
                    params![&item.display_name, clip.id],
                )
                .map_err(|error| error.to_string())?;
            }
            let clip = self.get_clip_with_conn(&tx, &clip.id)?;
            let total = self.clip_total_count_with_conn(&tx)?;
            tx.commit().map_err(|error| error.to_string())?;
            return Ok(Some((clip, total, false)));
        }

        let text = if item.clip_type == "image" {
            match item.image_bytes.as_deref() {
                Some(bytes) => self.save_image_bytes(&item.content_hash, bytes)?,
                None if item.text.starts_with("data:image/") => {
                    let bytes = image_bytes_from_data_url(&item.text)?;
                    self.save_image_bytes(&item.content_hash, &bytes)?
                }
                None => item.text,
            }
        } else {
            item.text
        };

        let last_captured_at = now();
        let clip = ClipItem {
            id: new_id(),
            clip_type: item.clip_type,
            content_hash: item.content_hash,
            display_name: item.display_name,
            preview_text: item.preview_text,
            text,
            source_app: None,
            last_captured_at,
            favorite_count: 0,
            is_pinned: false,
        };

        // INSERT + COUNT 同事务：图片文件已在上方落盘（IO 不占数据库写锁）。
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        tx.execute(
            "INSERT INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                clip.id,
                clip.clip_type,
                clip.content_hash,
                clip.display_name,
                clip.preview_text,
                clip.text,
                clip.source_app,
                clip.last_captured_at,
                clip.favorite_count,
                clip.is_pinned
            ],
        )
        .map_err(|error| error.to_string())?;
        let total = self.clip_total_count_with_conn(&tx)?;
        tx.commit().map_err(|error| error.to_string())?;

        Ok(Some((clip, total, true)))
    }

    pub(crate) fn upsert_append_copy_item(
        &self,
        clip_id: Option<String>,
        session_id: &str,
        text: String,
    ) -> Result<(ClipItem, usize, bool), String> {
        let conn = self.connect()?;
        let content_hash = hash_text(&format!("ipaste-append-copy:{session_id}:{text}"));
        let preview_text = preview(&text);
        let captured_at = now();

        if let Some(id) = clip_id.as_deref() {
            let active_exists = conn
                .query_row("SELECT id FROM clips WHERE id = ?1", params![id], |row| {
                    row.get::<_, String>(0)
                })
                .optional()
                .map_err(|error| error.to_string())?
                .is_some();

            if active_exists {
                conn.execute(
                    "UPDATE clips
                     SET clip_type = 'text',
                         content_hash = ?1,
                         preview_text = ?2,
                         text = ?3,
                         last_captured_at = ?4
                     WHERE id = ?5",
                    params![content_hash, preview_text, text, captured_at, id],
                )
                .map_err(|error| error.to_string())?;
                let clip = self.get_clip_with_conn(&conn, id)?;
                return Ok((clip, self.clip_total_count_with_conn(&conn)?, false));
            }
        }

        let clip = ClipItem {
            id: new_id(),
            clip_type: "text".to_string(),
            content_hash,
            display_name: Some("追加复制".to_string()),
            preview_text,
            text,
            source_app: None,
            last_captured_at: captured_at,
            favorite_count: 0,
            is_pinned: false,
        };

        conn.execute(
            "INSERT INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                clip.id,
                clip.clip_type,
                clip.content_hash,
                clip.display_name,
                clip.preview_text,
                clip.text,
                clip.source_app,
                clip.last_captured_at,
                clip.favorite_count,
                clip.is_pinned
            ],
        )
        .map_err(|error| error.to_string())?;

        Ok((clip, self.clip_total_count_with_conn(&conn)?, true))
    }

    pub(crate) fn prune_expired_with_conn(
        &self,
        conn: &Connection,
        retention_days: i64,
    ) -> Result<(), String> {
        // cutoff 与 now() 同为定长 "…Z" 格式：plain 字符串比较与原先的
        // datetime() 包裹完全等价，且能命中 idx_clips_last_captured_at
        // （datetime(col) 会让 DELETE 退化为全表扫描，5k 条实测 ~1.7ms/次）。
        let cutoff = (Utc::now() - ChronoDuration::days(retention_days))
            .to_rfc3339_opts(SecondsFormat::Secs, true);
        conn.execute(
            "DELETE FROM clips WHERE is_pinned = 0 AND last_captured_at < ?1",
            params![cutoff],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(crate) fn list_clips(
        &self,
        offset: usize,
        limit: usize,
        search: String,
        type_filter: String,
    ) -> Result<ClipPage, String> {
        let conn = self.connect()?;
        self.list_clips_page_with_conn(&conn, offset, limit, &search, &type_filter)
    }

    pub(super) fn list_clips_page_with_conn(
        &self,
        conn: &Connection,
        offset: usize,
        limit: usize,
        search: &str,
        type_filter: &str,
    ) -> Result<ClipPage, String> {
        let limit = limit.clamp(1, 100);
        let counts = self.clip_page_counts_with_conn(conn, search, type_filter)?;
        let all_count = self.clip_total_count_with_conn(conn)?;
        let (clips, has_more) =
            self.query_clips_page_with_conn(conn, offset, limit, search, type_filter)?;

        Ok(ClipPage {
            clips,
            has_more,
            total_count: counts.0,
            all_count,
            text_count: counts.1,
            image_count: counts.2,
        })
    }

    /// 历史页各计数：单次扫描同时算出 (total, text, image)。
    /// - total：应用搜索 + 类型筛选（分页总数 / 是否还有更多）；
    /// - text / image：只应用搜索（「全部/文本/图片」徽章在切换前就能展示
    ///   各自会有多少条，语义与原先的两次独立查询一致）。
    ///
    /// 原先 total 与 text/image 是两条独立的全表扫描查询，5k 条实测整页
    /// 读取 ~9.6ms（空搜索）/ ~22ms（带搜索词），且该路径在每次键入搜索时
    /// 都会触发；合并后只剩一次扫描。
    pub(super) fn clip_page_counts_with_conn(
        &self,
        conn: &Connection,
        search: &str,
        type_filter: &str,
    ) -> Result<(usize, usize, usize), String> {
        let query = search.trim().to_lowercase();
        let pattern = format!("%{query}%");
        let type_filter = clean_clip_type_filter(type_filter);

        if let Some(phrase) = fts_match_phrase(&query) {
            let binds: Vec<(&str, &dyn rusqlite::ToSql)> = vec![
                (":phrase", &phrase),
                (":pattern", &pattern),
                (":type_filter", &type_filter),
            ];
            let fts_counts = conn.query_row(
                "SELECT COALESCE(SUM(CASE WHEN (:type_filter = 'all'
                            OR (:type_filter = 'image' AND clip_type = 'image')
                            OR (:type_filter = 'text' AND clip_type != 'image'))
                        THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN clip_type != 'image' THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN clip_type = 'image' THEN 1 ELSE 0 END), 0)
                 FROM clips
                 WHERE (clips.rowid IN (SELECT rowid FROM clips_fts WHERE clips_fts MATCH :phrase)
                    OR lower(clip_type) LIKE :pattern
                    OR (clip_type = 'image' AND '图片 image' LIKE :pattern))",
                binds.as_slice(),
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? as usize,
                        row.get::<_, i64>(1)? as usize,
                        row.get::<_, i64>(2)? as usize,
                    ))
                },
            );
            match fts_counts {
                Ok(counts) => return Ok(counts),
                Err(reason) => {
                    eprintln!("[search] FTS 计数失败，回退 LIKE 扫描：{reason}");
                }
            }
        }

        conn.query_row(
            "SELECT COALESCE(SUM(CASE WHEN (?3 = 'all'
                        OR (?3 = 'image' AND clip_type = 'image')
                        OR (?3 = 'text' AND clip_type != 'image'))
                    THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN clip_type != 'image' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN clip_type = 'image' THEN 1 ELSE 0 END), 0)
             FROM clips
             WHERE (?1 = ''
                OR lower(COALESCE(display_name, '')) LIKE ?2
                OR lower(preview_text) LIKE ?2
                OR lower(clip_type) LIKE ?2
                OR (clip_type != 'image' AND lower(text) LIKE ?2)
                OR (clip_type = 'image' AND '图片 image' LIKE ?2))",
            params![query.as_str(), pattern.as_str(), type_filter.as_str()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as usize,
                    row.get::<_, i64>(1)? as usize,
                    row.get::<_, i64>(2)? as usize,
                ))
            },
        )
        .map_err(|error| error.to_string())
    }

    /// 分页行查询（不含计数）：ORDER BY 直接用列值。时间戳统一为 now() 的
    /// 定长 "…Z" 格式（见 migrations 的规范化），字典序即时间序，可命中
    /// idx_clips_last_captured_at；原先的 datetime() 包裹会退化为
    /// SCAN + TEMP B-TREE 全表排序（5k 条实测 7.6ms vs 索引序 0.07ms）。
    pub(super) fn query_clips_page_with_conn(
        &self,
        conn: &Connection,
        offset: usize,
        limit: usize,
        search: &str,
        type_filter: &str,
    ) -> Result<(Vec<ClipItem>, bool), String> {
        let query = search.trim().to_lowercase();
        let pattern = format!("%{query}%");
        let type_filter = clean_clip_type_filter(type_filter);

        if let Some(phrase) = fts_match_phrase(&query) {
            let page_limit = (limit + 1) as i64;
            let page_offset = offset as i64;
            let binds: Vec<(&str, &dyn rusqlite::ToSql)> = vec![
                (":phrase", &phrase),
                (":pattern", &pattern),
                (":type_filter", &type_filter),
                (":limit", &page_limit),
                (":offset", &page_offset),
            ];
            match self.query_clips_page_rows(
                conn,
                "SELECT id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned
                 FROM clips
                 WHERE (clips.rowid IN (SELECT rowid FROM clips_fts WHERE clips_fts MATCH :phrase)
                    OR lower(clip_type) LIKE :pattern
                    OR (clip_type = 'image' AND '图片 image' LIKE :pattern))
                   AND (:type_filter = 'all'
                    OR (:type_filter = 'image' AND clip_type = 'image')
                    OR (:type_filter = 'text' AND clip_type != 'image'))
                 ORDER BY last_captured_at DESC LIMIT :limit OFFSET :offset",
                binds.as_slice(),
                limit,
            ) {
                Ok(page) => return Ok(page),
                Err(reason) => {
                    eprintln!("[search] FTS 分页查询失败，回退 LIKE 扫描：{reason}");
                }
            }
        }

        let mut stmt = conn
            .prepare(
                "SELECT id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned
                 FROM clips
                 WHERE (?3 = ''
                    OR lower(COALESCE(display_name, '')) LIKE ?4
                    OR lower(preview_text) LIKE ?4
                    OR lower(clip_type) LIKE ?4
                    OR (clip_type != 'image' AND lower(text) LIKE ?4)
                    OR (clip_type = 'image' AND '图片 image' LIKE ?4))
                   AND (?5 = 'all'
                    OR (?5 = 'image' AND clip_type = 'image')
                    OR (?5 = 'text' AND clip_type != 'image'))
                 ORDER BY last_captured_at DESC LIMIT ?1 OFFSET ?2",
            )
            .map_err(|error| error.to_string())?;

        let rows = stmt
            .query_map(
                params![
                    (limit + 1) as i64,
                    offset as i64,
                    query.as_str(),
                    pattern.as_str(),
                    type_filter.as_str()
                ],
                map_clip,
            )
            .map_err(|error| error.to_string())?;

        let mut clips = collect_rows(rows)?;
        let has_more = clips.len() > limit;
        if has_more {
            clips.truncate(limit);
        }
        Ok((clips, has_more))
    }

    /// 命名参数版分页行查询（FTS 分支共用 LIKE 分支的行装配逻辑）。
    fn query_clips_page_rows(
        &self,
        conn: &Connection,
        sql: &str,
        binds: &[(&str, &dyn rusqlite::ToSql)],
        limit: usize,
    ) -> Result<(Vec<ClipItem>, bool), String> {
        let mut stmt = conn.prepare(sql).map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(binds, map_clip)
            .map_err(|error| error.to_string())?;
        let mut clips = collect_rows(rows)?;
        let has_more = clips.len() > limit;
        if has_more {
            clips.truncate(limit);
        }
        Ok((clips, has_more))
    }

    pub(super) fn clip_total_count_with_conn(&self, conn: &Connection) -> Result<usize, String> {
        conn.query_row("SELECT COUNT(*) FROM clips", [], |row| row.get::<_, i64>(0))
            .map(|count| count as usize)
            .map_err(|error| error.to_string())
    }

    /// 跨"历史/分类"搜索的统一入口：先查历史，历史无命中时回退到分类。
    /// 类型筛选同时约束两侧——历史命中与分类回退组看到的是同一个视图口径。
    /// 计数只算一遍：命中数与分页徽章复用同一次扫描（原先 count +
    /// list_clips_page 各扫一遍，等于把整页查询做了两次）。
    pub(crate) fn search_with_fallback(
        &self,
        offset: usize,
        limit: usize,
        search: &str,
        type_filter: &str,
    ) -> Result<SearchResult, String> {
        let conn = self.connect()?;
        let counts = self.clip_page_counts_with_conn(&conn, search, type_filter)?;
        if counts.0 > 0 {
            let limit = limit.clamp(1, 100);
            let (clips, has_more) =
                self.query_clips_page_with_conn(&conn, offset, limit, search, type_filter)?;
            let page = ClipPage {
                clips,
                has_more,
                total_count: counts.0,
                all_count: self.clip_total_count_with_conn(&conn)?,
                text_count: counts.1,
                image_count: counts.2,
            };
            Ok(SearchResult::History { page })
        } else {
            let groups = self.search_all_category_items_with_conn(&conn, search, type_filter)?;
            Ok(SearchResult::CategoryHits { groups })
        }
    }

    pub(crate) fn save_image_bytes(
        &self,
        content_hash: &str,
        bytes: &[u8],
    ) -> Result<String, String> {
        let dir = self.image_dir()?;
        let filename = format!("{}.png", safe_filename(content_hash));
        let path = dir.join(&filename);
        if !path.exists() {
            fs::write(&path, bytes).map_err(|error| error.to_string())?;
        }
        // 缩略图尽力而为：失败只让列表回退原图（前端 @error 兜底），
        // 不影响捕获/接收主路径的结果。
        if let Err(reason) = self.ensure_thumbnail(&dir, &filename, bytes) {
            eprintln!("[image] 生成缩略图失败（列表将回退原图）：{reason}");
        }
        Ok(path.to_string_lossy().to_string())
    }

    /// 生成缩略图：落在原图同目录的 thumbs/ 子目录、同名文件（前端按
    /// 「末段路径替换为 thumbs/<同名>」推导，无需改数据模型与事件契约）。
    /// 已存在即跳过（幂等），统一重编码为 PNG。
    fn ensure_thumbnail(
        &self,
        dir: &std::path::Path,
        filename: &str,
        bytes: &[u8],
    ) -> Result<(), String> {
        let thumb_dir = dir.join("thumbs");
        fs::create_dir_all(&thumb_dir).map_err(|error| error.to_string())?;
        let thumb_path = thumb_dir.join(filename);
        if thumb_path.exists() {
            return Ok(());
        }
        let image = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
        let thumb = if image.width() <= IMAGE_THUMBNAIL_MAX && image.height() <= IMAGE_THUMBNAIL_MAX
        {
            image
        } else {
            image.thumbnail(IMAGE_THUMBNAIL_MAX, IMAGE_THUMBNAIL_MAX)
        };
        thumb.save(&thumb_path).map_err(|error| error.to_string())
    }

    fn image_dir(&self) -> Result<PathBuf, String> {
        let dir = self
            .db_path
            .parent()
            .ok_or_else(|| "无法定位应用数据目录".to_string())?
            .join(IMAGE_DIR);
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        Ok(dir)
    }

    /// 升级后首启是否需要后台补齐存量图片缩略图（有落盘图片行且未置标志）。
    pub(crate) fn needs_image_thumbnail_backfill(
        &self,
        conn: &Connection,
    ) -> Result<bool, String> {
        let flagged: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'migrated_image_thumbnails'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if flagged.as_deref() == Some("1") {
            return Ok(false);
        }
        let image_rows: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM clips WHERE clip_type = 'image' AND text NOT LIKE 'data:%')
                      + (SELECT COUNT(*) FROM category_items WHERE clip_type = 'image' AND text NOT LIKE 'data:%')",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        Ok(image_rows > 0)
    }

    /// 存量缩略图补齐（后台线程执行）：逐张读取原图生成 thumbs/，完成置
    /// settings 标志。幂等且可中断——中途退出则标志未置位，下轮启动重跑；
    /// 与捕获线程并发生成同名文件时双方内容一致（同一哈希），无冲突。
    pub(crate) fn backfill_image_thumbnails(&self) {
        let conn = match self.connect() {
            Ok(conn) => conn,
            Err(reason) => {
                eprintln!("[image] 缩略图补齐无法连接数据库：{reason}");
                return;
            }
        };
        let mut stmt = match conn.prepare(
            "SELECT text FROM clips WHERE clip_type = 'image' AND text NOT LIKE 'data:%'
             UNION ALL
             SELECT text FROM category_items WHERE clip_type = 'image' AND text NOT LIKE 'data:%'",
        ) {
            Ok(stmt) => stmt,
            Err(reason) => {
                eprintln!("[image] 缩略图补齐查询失败：{reason}");
                return;
            }
        };
        let paths: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map(|rows| rows.filter_map(|row| row.ok()).collect())
            .unwrap_or_default();
        drop(stmt);

        let dir = match self.image_dir() {
            Ok(dir) => dir,
            Err(reason) => {
                eprintln!("[image] 缩略图补齐定位图片目录失败：{reason}");
                return;
            }
        };
        let mut failures = 0usize;
        for image_path in &paths {
            let path = PathBuf::from(image_path);
            let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(reason) => {
                    failures += 1;
                    if failures <= 3 {
                        eprintln!("[image] 缩略图补齐读取原图失败（{image_path}）：{reason}");
                    }
                    continue;
                }
            };
            if let Err(reason) = self.ensure_thumbnail(&dir, filename, &bytes) {
                failures += 1;
                if failures <= 3 {
                    eprintln!("[image] 缩略图补齐生成失败（{image_path}）：{reason}");
                }
            }
        }
        if let Err(reason) = conn.execute(
            "INSERT INTO settings (key, value) VALUES ('migrated_image_thumbnails', '1')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        ) {
            eprintln!("[image] 缩略图补齐标志写入失败：{reason}");
        }
    }

    /// 删除记录并返回其 content_hash（None = 记录不存在）。
    /// 命令层据此判断被删内容是否正是系统剪贴板当前内容，以联动清空。
    pub(crate) fn delete_clip_returning_hash(&self, id: String) -> Result<Option<String>, String> {
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let hash = tx
            .query_row(
                "SELECT content_hash FROM clips WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if hash.is_some() {
            tx.execute("DELETE FROM clips WHERE id = ?1", params![id])
                .map_err(|error| error.to_string())?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(hash)
    }

    pub(crate) fn clear_clips(&self) -> Result<usize, String> {
        let conn = self.connect()?;
        conn.execute("DELETE FROM clips", [])
            .map_err(|error| error.to_string())
    }

    pub(crate) fn rename_clip(
        &self,
        id: String,
        collection: String,
        display_name: Option<String>,
    ) -> Result<ClipUpdate, String> {
        let display_name = clean_display_name(display_name)?;
        let conn = self.connect()?;
        match collection.as_str() {
            "history" => {
                conn.execute(
                    "UPDATE clips SET display_name = ?1 WHERE id = ?2",
                    params![display_name, id],
                )
                .map_err(|error| error.to_string())?;
                self.get_clip_with_conn(&conn, &id).map(ClipUpdate::Clip)
            }
            "category" => {
                conn.execute(
                    "UPDATE category_items SET display_name = ?1, updated_at = ?2 WHERE id = ?3",
                    params![display_name, now(), id],
                )
                .map_err(|error| error.to_string())?;
                self.get_category_item_with_conn(&conn, &id)
                    .map(ClipUpdate::CategoryItem)
            }
            _ => Err("未知条目来源".to_string()),
        }
    }

    pub(crate) fn update_clip_content(
        &self,
        id: String,
        collection: String,
        text: String,
    ) -> Result<ClipUpdate, String> {
        let preview_text = preview(&text);
        let content_hash = hash_text(&text);
        match collection.as_str() {
            "history" => {
                let mut conn = self.connect()?;
                let tx = conn.transaction().map_err(|error| error.to_string())?;
                let current = self.get_clip_with_conn(&tx, &id)?;

                if let Some(existing) =
                    self.get_clip_by_content_hash_with_conn(&tx, &content_hash, Some(&id))?
                {
                    let existing_id = existing.id.clone();
                    let last_captured_at = now();
                    let display_name = existing.display_name.clone().or(current.display_name);
                    let is_pinned = existing.is_pinned || current.is_pinned;
                    let favorite_count = existing.favorite_count + current.favorite_count;
                    tx.execute(
                        "UPDATE clips
                         SET display_name = ?1, favorite_count = ?2, is_pinned = ?3, last_captured_at = ?4
                         WHERE id = ?5",
                        params![display_name, favorite_count, is_pinned, last_captured_at, existing_id],
                    )
                    .map_err(|error| error.to_string())?;
                    tx.execute(
                        "UPDATE category_items SET clip_snapshot_id = ?1 WHERE clip_snapshot_id = ?2",
                        params![existing_id, id],
                    )
                    .map_err(|error| error.to_string())?;
                    tx.execute("DELETE FROM clips WHERE id = ?1", params![id])
                        .map_err(|error| error.to_string())?;
                    let clip = self.get_clip_with_conn(&tx, &existing_id)?;
                    tx.commit().map_err(|error| error.to_string())?;
                    return Ok(ClipUpdate::Clip(clip));
                }

                tx.execute(
                    "UPDATE clips SET text = ?1, preview_text = ?2, content_hash = ?3 WHERE id = ?4",
                    params![text, preview_text, content_hash, id],
                )
                .map_err(|error| error.to_string())?;
                let clip = self.get_clip_with_conn(&tx, &id)?;
                tx.commit().map_err(|error| error.to_string())?;
                Ok(ClipUpdate::Clip(clip))
            }
            "category" => {
                let mut conn = self.connect()?;
                let tx = conn.transaction().map_err(|error| error.to_string())?;
                let current = self.get_category_item_with_conn(&tx, &id)?;

                if let Some(existing) = self.get_category_item_by_content_hash_with_conn(
                    &tx,
                    &current.category_id,
                    &content_hash,
                    Some(&id),
                )? {
                    self.record_tombstone_with_conn(&tx, "category_item", &id)?;
                    tx.execute("DELETE FROM category_items WHERE id = ?1", params![id])
                        .map_err(|error| error.to_string())?;
                    tx.commit().map_err(|error| error.to_string())?;
                    return Ok(ClipUpdate::CategoryItem(existing));
                }

                tx.execute(
                    "UPDATE category_items SET text = ?1, preview_text = ?2, content_hash = ?3, sync_state = 'local', updated_at = ?4 WHERE id = ?5",
                    params![text, preview_text, content_hash, now(), id],
                )
                .map_err(|error| error.to_string())?;
                let item = self.get_category_item_with_conn(&tx, &id)?;
                tx.commit().map_err(|error| error.to_string())?;
                Ok(ClipUpdate::CategoryItem(item))
            }
            _ => Err("未知条目来源".to_string()),
        }
    }

    pub(crate) fn set_clip_pinned(
        &self,
        id: String,
        collection: String,
        is_pinned: bool,
    ) -> Result<ClipUpdate, String> {
        let conn = self.connect()?;
        match collection.as_str() {
            "history" => {
                conn.execute(
                    "UPDATE clips SET is_pinned = ?1 WHERE id = ?2",
                    params![is_pinned, id],
                )
                .map_err(|error| error.to_string())?;
                self.get_clip_with_conn(&conn, &id).map(ClipUpdate::Clip)
            }
            "category" => {
                conn.execute(
                    "UPDATE category_items SET is_pinned = ?1, updated_at = ?2 WHERE id = ?3",
                    params![is_pinned, now(), id],
                )
                .map_err(|error| error.to_string())?;
                self.get_category_item_with_conn(&conn, &id)
                    .map(ClipUpdate::CategoryItem)
            }
            _ => Err("未知条目来源".to_string()),
        }
    }

    pub(crate) fn get_clip_with_conn(
        &self,
        conn: &Connection,
        id: &str,
    ) -> Result<ClipItem, String> {
        conn.query_row(
            "SELECT id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned
             FROM clips WHERE id = ?1",
            params![id],
            map_clip,
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "未找到剪贴板记录".to_string())
    }

    fn get_clip_by_content_hash_with_conn(
        &self,
        conn: &Connection,
        content_hash: &str,
        exclude_id: Option<&str>,
    ) -> Result<Option<ClipItem>, String> {
        conn.query_row(
            "SELECT id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned
             FROM clips
             WHERE content_hash = ?1 AND (?2 IS NULL OR id != ?2)",
            params![content_hash, exclude_id],
            map_clip,
        )
        .optional()
        .map_err(|error| error.to_string())
    }

    /// 应用条目时刷新其最近捕获时间（原 commands.rs 中的裸 SQL，收编入 store 层）。
    pub(crate) fn touch_clip_captured(&self, id: &str) -> Result<(), String> {
        let conn = self.connect()?;
        self.touch_clip_captured_with_conn(&conn, id)
    }

    pub(crate) fn touch_clip_captured_with_conn(
        &self,
        conn: &Connection,
        id: &str,
    ) -> Result<(), String> {
        conn.execute(
            "UPDATE clips SET last_captured_at = ?1 WHERE id = ?2",
            params![now(), id],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{CapturedClipboardItem, SearchResult};
    use crate::store::test_support::{seed_clip, seed_n_clips, temp_store};
    use crate::util::hash_text;
    use rusqlite::params;

    /// 构造一张 PNG 的内存字节（宽高可超过缩略图上限）。
    fn test_png_bytes(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(width, height, |x, _| {
            image::Rgba([(x % 256) as u8, 0x7f, 0xa5, 0xff])
        });
        let mut cursor = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut cursor, image::ImageFormat::Png)
            .expect("测试 PNG 编码失败");
        cursor.into_inner()
    }

    #[test]
    fn save_image_bytes_writes_scaled_thumbnail() {
        let store = temp_store();
        let png = test_png_bytes(600, 300);
        let stored = store.save_image_bytes("thumb-hash-1", &png).unwrap();
        let original = std::path::PathBuf::from(&stored);
        let thumb = original
            .parent()
            .unwrap()
            .join("thumbs")
            .join(original.file_name().unwrap());
        assert!(thumb.exists(), "缩略图应生成在原图同目录的 thumbs/ 下");
        let decoded = image::open(&thumb).unwrap();
        // 等比缩放到最长边 480：600×300 → 480×240。
        assert_eq!((decoded.width(), decoded.height()), (480, 240));
        // 幂等：重复保存同名内容不报错、不重复生成。
        store.save_image_bytes("thumb-hash-1", &png).unwrap();
    }

    #[test]
    fn thumbnail_backfill_covers_existing_rows_and_sets_flag() {
        let store = temp_store();
        // 绕过 save_image_bytes（它会即时生成缩略图），直接落原图 + 插库行，
        // 模拟「升级前已存在的存量图片」。
        let png = test_png_bytes(800, 600);
        let dir = store.image_dir().unwrap();
        let original = dir.join("legacy-image.png");
        std::fs::write(&original, &png).unwrap();
        {
            let conn = store.connect().unwrap();
            conn.execute(
                "INSERT INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
                 VALUES ('backfill-1', 'image', 'hash-backfill-1', NULL, '图片', ?1, NULL, '2024-01-01T00:00:00Z', 0, 0)",
                params![original.to_string_lossy().to_string()],
            )
            .unwrap();
        }

        let conn = store.connect().unwrap();
        assert!(store.needs_image_thumbnail_backfill(&conn).unwrap());
        drop(conn);

        store.backfill_image_thumbnails();

        let thumb = dir.join("thumbs").join("legacy-image.png");
        assert!(thumb.exists(), "存量图片应被补齐缩略图");
        let decoded = image::open(&thumb).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (480, 360));
        let conn = store.connect().unwrap();
        assert!(
            !store.needs_image_thumbnail_backfill(&conn).unwrap(),
            "补齐完成后标志应置位"
        );
    }

    #[test]
    fn thumbnail_backfill_not_needed_for_fresh_library() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_clip(&conn, "text", "plain text", "plain text");
        assert!(
            !store.needs_image_thumbnail_backfill(&conn).unwrap(),
            "无落盘图片行的全新库不需要补齐"
        );
    }

    use std::time::Instant;

    #[test]
    fn delete_clip_returns_content_hash_of_deleted_row() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_clip(&conn, "text", "hello", "hello world");
        let id: String = conn
            .query_row("SELECT id FROM clips LIMIT 1", [], |row| row.get(0))
            .unwrap();

        let hash = store.delete_clip_returning_hash(id.clone()).unwrap();
        assert_eq!(hash.as_deref(), Some(hash_text("hello world").as_str()));

        let gone = store.delete_clip_returning_hash(id).unwrap();
        assert_eq!(gone, None, "记录不存在时应返回 None 而非报错");
    }

    #[test]
    fn touch_clip_captured_updates_timestamp() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_clip(&conn, "text", "touch-me", "body");
        let id: String = conn
            .query_row("SELECT id FROM clips LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let before: String = conn
            .query_row(
                "SELECT last_captured_at FROM clips WHERE id = ?1",
                params![&id],
                |row| row.get(0),
            )
            .unwrap();
        // now() 为秒级精度（SecondsFormat::Secs），必须跨过秒边界才能观测变化。
        std::thread::sleep(std::time::Duration::from_millis(1100));
        store.touch_clip_captured_with_conn(&conn, &id).unwrap();
        let after: String = conn
            .query_row(
                "SELECT last_captured_at FROM clips WHERE id = ?1",
                params![&id],
                |row| row.get(0),
            )
            .unwrap();
        assert_ne!(before, after, "touch 应更新 last_captured_at");
    }

    #[test]
    fn fts_search_tracks_insert_update_and_delete() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_clip(&conn, "text", "release notes for version two", "release notes");
        seed_clip(&conn, "text", "unrelated grocery list", "grocery list");

        // ≥3 字符走 FTS：命中包含查询串的行。
        let (total, _, _) = store
            .clip_page_counts_with_conn(&conn, "release notes", "all")
            .unwrap();
        assert_eq!(total, 1, "FTS 短语应命中插入的正文");
        let (clips, _) = store
            .query_clips_page_with_conn(&conn, 0, 20, "release notes", "all")
            .unwrap();
        assert_eq!(clips.len(), 1);

        // 更新正文（触发器同步索引）：新词命中、旧词消失。
        conn.execute(
            "UPDATE clips SET text = 'rebuilt changelog body', preview_text = 'rebuilt changelog'
             WHERE text LIKE 'release notes%'",
            [],
        )
        .unwrap();
        let (total_new, _, _) = store
            .clip_page_counts_with_conn(&conn, "changelog", "all")
            .unwrap();
        assert_eq!(total_new, 1, "更新后的新词应可搜到");
        let (total_old, _, _) = store
            .clip_page_counts_with_conn(&conn, "release notes", "all")
            .unwrap();
        assert_eq!(total_old, 0, "更新后旧词不应再命中");

        // 删除行：索引同步清除。
        conn.execute("DELETE FROM clips WHERE preview_text = 'rebuilt changelog'", [])
            .unwrap();
        let (total_after_delete, _, _) = store
            .clip_page_counts_with_conn(&conn, "changelog", "all")
            .unwrap();
        assert_eq!(total_after_delete, 0);
    }

    #[test]
    fn fts_search_image_rows_match_label_but_not_file_path() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        // 注意 seed_clip 参数顺序是 (preview, text)：路径进 text 列。
        seed_clip(&conn, "image", "图片 800×600", "C:/data/clip-images/deadbeef.png");

        // 图片的 text（文件路径）不应被搜到——与 LIKE 语义一致。
        let (by_path, _, _) = store
            .clip_page_counts_with_conn(&conn, "deadbeef", "all")
            .unwrap();
        assert_eq!(by_path, 0, "图片文件路径不应参与匹配");
        // 「图片」标签与 clip_type 仍然可命中（三字以上走 FTS 时的 OR 条件）。
        let (by_label, _, image_count) = store
            .clip_page_counts_with_conn(&conn, "图片", "all")
            .unwrap();
        assert_eq!((by_label, image_count), (1, 1));
    }

    #[test]
    fn short_queries_fall_back_to_like_and_still_match() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_clip(&conn, "text", "会议纪要 today", "会议纪要");

        // 两字中文词（trigram 最短 3 字符）回退 LIKE，语义不变。
        let (total, _, _) = store.clip_page_counts_with_conn(&conn, "会议", "all").unwrap();
        assert_eq!(total, 1);
        let (clips, _) = store
            .query_clips_page_with_conn(&conn, 0, 20, "会议", "all")
            .unwrap();
        assert_eq!(clips.len(), 1);
    }

    #[test]
    fn clip_page_counts_respect_query() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_clip(&conn, "text", "hello world", "hello world");
        seed_clip(&conn, "text", "rust lang", "rust lang");
        let (total, text, image) = store
            .clip_page_counts_with_conn(&conn, "hello", "all")
            .unwrap();
        assert_eq!((total, text, image), (1, 1, 0));
        let (total, _, _) = store
            .clip_page_counts_with_conn(&conn, "", "all")
            .unwrap();
        assert_eq!(total, 2);
        let (total, _, _) = store
            .clip_page_counts_with_conn(&conn, "nomatch", "all")
            .unwrap();
        assert_eq!(total, 0);
    }

    #[test]
    fn list_clips_type_filter_splits_image_and_text() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_clip(&conn, "text", "hello", "hello");
        seed_clip(&conn, "link", "https://example.com", "https://example.com");
        seed_clip(&conn, "image", "图片预览", "/tmp/ipaste-image.png");

        let image_page = store
            .list_clips_page_with_conn(&conn, 0, 20, "", "image")
            .unwrap();
        assert_eq!(image_page.clips.len(), 1);
        assert_eq!(image_page.clips[0].clip_type, "image");
        assert_eq!(image_page.total_count, 1);
        // 徽章计数不受类型筛选影响：三段都能看到各自条数。
        assert_eq!(image_page.text_count, 2);
        assert_eq!(image_page.image_count, 1);

        let text_page = store
            .list_clips_page_with_conn(&conn, 0, 20, "", "text")
            .unwrap();
        assert_eq!(text_page.clips.len(), 2);
        assert!(text_page.clips.iter().all(|clip| clip.clip_type != "image"));

        // 未知筛选值归一为 all（不过滤）。
        let all_page = store
            .list_clips_page_with_conn(&conn, 0, 20, "", "bogus")
            .unwrap();
        assert_eq!(all_page.clips.len(), 3);
    }

    #[test]
    fn list_clips_type_filter_composes_with_search() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_clip(&conn, "text", "hello", "hello");
        seed_clip(&conn, "image", "图片预览", "/tmp/ipaste-image.png");

        // 搜索词命中文本、但筛选只要图片 → 空。
        let page = store
            .list_clips_page_with_conn(&conn, 0, 20, "hello", "image")
            .unwrap();
        assert_eq!(page.clips.len(), 0);
        assert_eq!(page.total_count, 0);
        // 计数仍跟随搜索上下文（hello 只命中文本）。
        assert_eq!(page.text_count, 1);
        assert_eq!(page.image_count, 0);

        // 图片可通过 "image" 关键字命中，配合图片筛选成立。
        let page = store
            .list_clips_page_with_conn(&conn, 0, 20, "image", "image")
            .unwrap();
        assert_eq!(page.clips.len(), 1);
        assert_eq!(page.clips[0].clip_type, "image");
    }

    #[test]
    fn fallback_type_filter_applies_to_category_hits() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        let cat = crate::store::test_support::create_category(&conn, "A", "#f00", 0);
        crate::store::test_support::seed_category_item(
            &conn,
            &cat,
            "text",
            "secret token",
            "secret token",
        );
        crate::store::test_support::seed_category_item(
            &conn,
            &cat,
            "image",
            "secret image",
            "/tmp/secret.png",
        );

        // 历史无命中，回退到分类；图片筛选只保留图片条目。
        let res = store
            .search_with_fallback(0, 20, "secret", "image")
            .unwrap();
        match res {
            SearchResult::CategoryHits { groups } => {
                assert_eq!(groups.len(), 1);
                assert_eq!(groups[0].items.len(), 1);
                assert_eq!(groups[0].items[0].clip_type, "image");
            }
            other => panic!("expected CategoryHits, got {:?}", other),
        }

        // 文本筛选下图片条目被滤掉。
        let res = store
            .search_with_fallback(0, 20, "secret", "text")
            .unwrap();
        match res {
            SearchResult::CategoryHits { groups } => {
                assert_eq!(groups.len(), 1);
                assert_eq!(groups[0].items.len(), 1);
                assert_eq!(groups[0].items[0].clip_type, "text");
            }
            other => panic!("expected CategoryHits, got {:?}", other),
        }
    }

    #[test]
    fn fallback_returns_history_when_history_has_hits() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_clip(&conn, "text", "hello", "hello");
        seed_clip(&conn, "image", "图片预览", "/tmp/ipaste-image.png");
        let res = store.search_with_fallback(0, 20, "image", "all").unwrap();
        match res {
            SearchResult::History { page } => assert_eq!(page.clips.len(), 1),
            other => panic!("expected History, got {:?}", other),
        }
    }

    #[test]
    fn fallback_returns_category_hits_when_history_empty() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        let cat = crate::store::test_support::create_category(&conn, "A", "#f00", 0);
        crate::store::test_support::seed_category_item(
            &conn,
            &cat,
            "text",
            "secret token",
            "secret token",
        );
        let res = store.search_with_fallback(0, 20, "secret", "all").unwrap();
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
        let res = store.search_with_fallback(0, 20, "ghost", "all").unwrap();
        match res {
            SearchResult::CategoryHits { groups } => assert!(groups.is_empty()),
            other => panic!("expected CategoryHits, got {:?}", other),
        }
    }

    #[test]
    fn insert_captured_item_merges_on_duplicate_content_hash() {
        let store = temp_store();
        let item = CapturedClipboardItem {
            clip_type: "text".to_string(),
            content_hash: hash_text("hello"),
            preview_text: "hello".to_string(),
            text: "hello".to_string(),
            image_bytes: None,
            display_name: None,
        };
        let first = store.insert_captured_item(item.clone()).unwrap().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let second = store.insert_captured_item(item).unwrap().unwrap();

        assert!(first.2, "first insert should report was_inserted=true");
        assert!(!second.2, "duplicate should report was_inserted=false");
        assert_eq!(first.1, 1);
        assert_eq!(second.1, 1);
        assert!(second.0.last_captured_at >= first.0.last_captured_at);

        let conn = store.connect().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clips WHERE content_hash = ?1",
                rusqlite::params![hash_text("hello")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn list_clips_first_page_1k_under_250ms() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_n_clips(&conn, 1000);
        let start = Instant::now();
        let _ = store
            .list_clips(0, 20, "".to_string(), "all".to_string())
            .unwrap();
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 250, "list_clips 1k took {elapsed:?}");
    }

    #[test]
    fn list_clips_first_page_5k_under_750ms() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_n_clips(&conn, 5000);
        let start = Instant::now();
        let _ = store
            .list_clips(0, 20, "".to_string(), "all".to_string())
            .unwrap();
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 750, "list_clips 5k took {elapsed:?}");
    }

    #[test]
    fn search_with_fallback_5k_under_1000ms() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_n_clips(&conn, 5000);
        let start = Instant::now();
        let _ = store.search_with_fallback(0, 20, "hello", "all").unwrap();
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 1000, "search 5k took {elapsed:?}");
    }
}
