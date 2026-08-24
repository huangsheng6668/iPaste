//! 设备身份：iroh SecretKey 的钥匙串持久化与 load-or-create。

use iroh::SecretKey;

use crate::store::secrets::{get_device_secret, put_device_secret};

/// 加载或生成设备身份私钥。首次调用生成并写入钥匙串；此后每次启动加载同一把
/// （EndpointId 稳定 = 已配对设备凭它认得我们）。钥匙串内容损坏时显式报错——
/// 静默重新生成会让所有已配对设备把我们当陌生人。
pub(crate) fn load_or_create_device_secret() -> Result<SecretKey, String> {
    if let Some(hex_str) = get_device_secret()? {
        let bytes = decode_hex_32(&hex_str)?;
        return Ok(SecretKey::from(bytes));
    }
    let key = SecretKey::generate();
    put_device_secret(&encode_hex(&key.to_bytes()))?;
    Ok(key)
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_hex_32(input: &str) -> Result<[u8; 32], String> {
    if input.len() != 64 || !input.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("设备私钥存储格式损坏".to_string());
    }
    let mut out = [0u8; 32];
    for (i, chunk) in input.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16).ok_or("设备私钥存储格式损坏")?;
        let lo = (chunk[1] as char).to_digit(16).ok_or("设备私钥存储格式损坏")?;
        out[i] = (hi * 16 + lo) as u8;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::secrets::delete_device_secret;
    use crate::store::test_support::temp_store;

    #[test]
    fn load_or_create_is_stable_across_calls() {
        let _store = temp_store();
        delete_device_secret().unwrap();
        let a = load_or_create_device_secret().unwrap();
        let b = load_or_create_device_secret().unwrap();
        assert_eq!(a.to_bytes(), b.to_bytes(), "第二次加载必须得到同一把私钥");
    }

    #[test]
    fn corrupted_secret_fails_loudly() {
        let _store = temp_store();
        delete_device_secret().unwrap();
        put_device_secret("zzzz").unwrap();
        assert!(load_or_create_device_secret().is_err(), "损坏的私钥必须报错而非静默换新");
    }
}
