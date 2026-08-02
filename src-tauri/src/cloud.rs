use std::collections::HashSet;
use std::time::Duration;

use reqwest::{blocking::Client, StatusCode};
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::models::*;

pub(crate) fn ensure_unique_ids(ids: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for id in ids {
        let id = id.trim();
        if id.is_empty() {
            return Err("排序 ID 不能为空".to_string());
        }
        if !seen.insert(id.to_string()) {
            return Err("排序列表包含重复条目".to_string());
        }
    }
    Ok(())
}

pub(crate) fn ensure_category_exists(conn: &Connection, category_id: &str) -> Result<(), String> {
    conn.query_row(
        "SELECT id FROM categories WHERE id = ?1",
        params![category_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|error| error.to_string())?
    .map(|_| ())
    .ok_or_else(|| "未找到分类".to_string())
}

pub(crate) fn ensure_all_categories_exist(conn: &Connection, category_ids: &[String]) -> Result<(), String> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM categories", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if count as usize != category_ids.len() {
        return Err("分类顺序需要包含全部分类".to_string());
    }

    for id in category_ids {
        ensure_category_exists(conn, id)?;
    }
    Ok(())
}

pub(crate) fn ensure_all_category_items_exist(
    conn: &Connection,
    category_id: &str,
    item_ids: &[String],
) -> Result<(), String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM category_items WHERE category_id = ?1",
            params![category_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if count as usize != item_ids.len() {
        return Err("条目顺序需要包含该分类下的全部条目".to_string());
    }

    for id in item_ids {
        conn.query_row(
            "SELECT id FROM category_items WHERE id = ?1 AND category_id = ?2",
            params![id, category_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "排序列表包含不属于该分类的条目".to_string())?;
    }
    Ok(())
}

pub(crate) fn cloud_get<T: DeserializeOwned>(
    api_address: &str,
    api_key: &str,
    path: &str,
) -> Result<T, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(format!("{api_address}{path}"))
        .bearer_auth(api_key)
        .send()
        .map_err(|error| error.to_string())?;

    parse_cloud_response(response)
}

pub(crate) fn cloud_post<T: DeserializeOwned, B: Serialize>(
    api_address: &str,
    api_key: &str,
    path: &str,
    body: &B,
) -> Result<T, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .post(format!("{api_address}{path}"))
        .bearer_auth(api_key)
        .json(body)
        .send()
        .map_err(|error| error.to_string())?;

    parse_cloud_response(response)
}

pub(crate) fn parse_cloud_response<T: DeserializeOwned>(
    response: reqwest::blocking::Response,
) -> Result<T, String> {
    let status = response.status();
    let envelope = response
        .json::<CloudEnvelope<T>>()
        .map_err(|error| format!("无法解析云同步响应：{error}"))?;

    if !status.is_success() || envelope.ok == Some(false) {
        return Err(envelope
            .error
            .unwrap_or_else(|| cloud_status_message(status)));
    }

    Ok(envelope.data)
}

pub(crate) fn cloud_status_message(status: StatusCode) -> String {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            "云同步认证失败，请检查 API Key".to_string()
        }
        _ => format!("云同步请求失败：{}", status.as_u16()),
    }
}
