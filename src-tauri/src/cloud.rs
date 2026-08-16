use std::time::Duration;

use reqwest::{blocking::Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::models::{CloudEnvelope, HealthPayload};

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
