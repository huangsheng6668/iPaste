use std::io::Read;
use std::time::Duration;

use reqwest::{blocking::Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::models::{CloudEnvelope, HealthPayload};

/// 云同步响应体体积上限：api_address 由用户配置且允许 http 明文，
/// 必须限制单次读入内存的字节数，防止恶意/被劫持的服务端推送超大 JSON 导致 OOM。
const CLOUD_MAX_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;

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
    if let Some(length) = response.content_length() {
        if length > CLOUD_MAX_RESPONSE_BYTES {
            return Err("云同步响应过大，已拒绝读取".to_string());
        }
    }

    let mut body = Vec::new();
    let mut limited = response.take(CLOUD_MAX_RESPONSE_BYTES + 1);
    limited
        .read_to_end(&mut body)
        .map_err(|error| format!("无法读取云同步响应：{error}"))?;
    if body.len() as u64 > CLOUD_MAX_RESPONSE_BYTES {
        return Err("云同步响应过大，已拒绝读取".to_string());
    }

    let envelope = serde_json::from_slice::<CloudEnvelope<T>>(&body)
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

const CLOUD_SYNC_TYPES: [&str; 4] = ["text", "link", "color", "html"];

pub(crate) fn test_cloud_connection(api_address: &str, api_key: &str) -> Result<(), String> {
    let payload: HealthPayload = cloud_get(api_address, api_key, "/api/health")?;
    if payload.service.as_deref() == Some("ipaste-cloud") {
        Ok(())
    } else {
        Err("云同步服务响应不正确".to_string())
    }
}

pub(crate) fn is_syncable_clip_type(clip_type: &str) -> bool {
    CLOUD_SYNC_TYPES.contains(&clip_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, serde::Deserialize)]
    struct Probe {
        value: i32,
    }

    fn response_with_body(body: Vec<u8>) -> reqwest::blocking::Response {
        let http_response = http::Response::builder().status(200).body(body).unwrap();
        reqwest::blocking::Response::from(http_response)
    }

    #[test]
    fn parse_cloud_response_accepts_normal_body() {
        let body = br#"{"ok":true,"value":7}"#.to_vec();
        let parsed: Probe = parse_cloud_response(response_with_body(body)).unwrap();
        assert_eq!(parsed.value, 7);
    }

    #[test]
    fn parse_cloud_response_rejects_oversized_body() {
        let body = vec![b'x'; CLOUD_MAX_RESPONSE_BYTES as usize + 1];
        let error = parse_cloud_response::<Probe>(response_with_body(body))
            .expect_err("超大响应体应被拒绝");
        assert!(error.contains("响应过大"), "got: {error}");
    }

    #[test]
    fn parse_cloud_response_allows_body_at_limit() {
        let mut body = br#"{"ok":true,"value":7}"#.to_vec();
        body.resize(CLOUD_MAX_RESPONSE_BYTES as usize, b' ');
        let result: Result<Probe, String> = parse_cloud_response(response_with_body(body));
        // 恰好等于上限的体积不触发「过大」拒绝（JSON 尾随空格合法，应解析成功）
        assert_eq!(result.unwrap().value, 7);
    }
}
