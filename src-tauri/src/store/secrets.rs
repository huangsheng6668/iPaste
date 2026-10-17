//! 云 API Key 的系统凭据库存取（Windows Credential Manager / macOS Keychain）。
//! 测试统一切换到进程内内存 mock（见 `use_mock_backend_for_tests`），
//! 不触碰真实系统库。

use keyring::Entry;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const SERVICE: &str = "iPaste";
const ACCOUNT: &str = "cloud_api_key";
const ACCOUNT_DEVICE: &str = "device_sync_secret";
const ACCOUNT_OPENAI_OCR: &str = "openai_ocr_api_key";

/// 进程内读缓存。凭据库单次读取是跨进程 IPC（Windows 凭据管理器尤其明显），
/// 而每次读设置（设置面板打开、状态刷新）都会回填 Key——把最近一次成功读到的
/// 值按账户名缓存，写入口（put_*/delete_*）落库后立即失效对应条目，保证进程内
/// 读写一致。失败读取不缓存（允许下轮重试）；这些条目的唯一写方就是本 app
/// （设置保存 / 云同步开关），外部进程改库不在处理范围。
fn cache() -> &'static Mutex<HashMap<&'static str, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn entry_for(account: &str) -> Result<Entry, String> {
    keyring::Entry::new(SERVICE, account).map_err(|e| format!("无法访问系统凭据库：{e}"))
}

/// 读缓存 → 未命中回源凭据库；仅成功结果入缓存。
fn read_cached(account: &'static str) -> Result<Option<String>, String> {
    if let Some(hit) = cache()
        .lock()
        .expect("secrets 缓存锁中毒")
        .get(account)
        .cloned()
    {
        return Ok(hit);
    }
    let value = match entry_for(account)?.get_password() {
        Ok(v) => Some(v),
        Err(keyring::Error::NoEntry) => None,
        Err(e) => return Err(format!("读取系统凭据库失败：{e}")),
    };
    cache()
        .lock()
        .expect("secrets 缓存锁中毒")
        .insert(account, value.clone());
    Ok(value)
}

/// 写路径调用：丢弃缓存条目，下次读重新回源。删除即使失败也失效——
/// 缓存只是读穿透层，失效最多多付一次读取，不会返回与库不符的值。
fn invalidate_cached(account: &'static str) {
    cache().lock().expect("secrets 缓存锁中毒").remove(account);
}

pub(crate) fn put_api_key(value: &str) -> Result<(), String> {
    entry_for(ACCOUNT)?
        .set_password(value)
        .map_err(|e| format!("写入系统凭据库失败：{e}"))?;
    invalidate_cached(ACCOUNT);
    Ok(())
}

pub(crate) fn get_api_key() -> Result<Option<String>, String> {
    read_cached(ACCOUNT)
}

/// 幂等删除：条目不存在视为成功（如从未配置过云同步）。
pub(crate) fn delete_api_key() -> Result<(), String> {
    let result = match entry_for(ACCOUNT)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除系统凭据库条目失败：{e}")),
    };
    invalidate_cached(ACCOUNT);
    result
}

pub(crate) fn put_device_secret(value: &str) -> Result<(), String> {
    entry_for(ACCOUNT_DEVICE)?
        .set_password(value)
        .map_err(|e| format!("写入系统凭据库失败：{e}"))?;
    invalidate_cached(ACCOUNT_DEVICE);
    Ok(())
}

pub(crate) fn get_device_secret() -> Result<Option<String>, String> {
    read_cached(ACCOUNT_DEVICE)
}

/// 测试清理用；生产代码不调用（设备身份条目跟随 app 卸载清除）。
#[cfg(test)]
pub(crate) fn delete_device_secret() -> Result<(), String> {
    let result = match entry_for(ACCOUNT_DEVICE)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除系统凭据库条目失败：{e}")),
    };
    invalidate_cached(ACCOUNT_DEVICE);
    result
}

/// 清理已移除的智谱专用 OCR 引擎（≤0.9.9）遗留的凭据库条目。
/// 幂等、尽力而为：条目不存在或凭据库不可用都直接忽略。
/// 该账户没有读入口（不缓存），无需失效。
pub(crate) fn remove_legacy_bigmodel_api_key() {
    if let Ok(entry) = keyring::Entry::new(SERVICE, "bigmodel_api_key") {
        let _ = entry.delete_credential();
    }
}

/// 通用 OpenAI 兼容接口的 API Key（与智谱 Key 分账户，切换引擎互不影响）。
pub(crate) fn put_openai_ocr_api_key(value: &str) -> Result<(), String> {
    entry_for(ACCOUNT_OPENAI_OCR)?
        .set_password(value)
        .map_err(|e| format!("写入系统凭据库失败：{e}"))?;
    invalidate_cached(ACCOUNT_OPENAI_OCR);
    Ok(())
}

pub(crate) fn get_openai_ocr_api_key() -> Result<Option<String>, String> {
    read_cached(ACCOUNT_OPENAI_OCR)
}

/// 幂等删除（从未配置过视为成功）。
pub(crate) fn delete_openai_ocr_api_key() -> Result<(), String> {
    let result = match entry_for(ACCOUNT_OPENAI_OCR)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除系统凭据库条目失败：{e}")),
    };
    invalidate_cached(ACCOUNT_OPENAI_OCR);
    result
}

/// 测试专用：把默认凭据后端切换为进程内内存 mock（幂等，进程内一次）。
/// 由 `test_support::temp_store` 统一调用，保证所有 store 测试不触真实系统库，
/// 也让 Linux CI（无凭据后端）能跑通。
///
/// 注：keyring 自带的 `mock::default_credential_builder()` 是 EntryOnly 语义——
/// 状态存在 Entry 对象里，而本模块每次读写都新建 Entry，无法跨调用回读。
/// 故自行实现一个共享 HashMap 的 mock 后端，语义与真实凭据库一致。
#[cfg(test)]
pub(crate) fn use_mock_backend_for_tests() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        keyring::set_default_credential_builder(Box::new(mock_backend::MapStoreBuilder));
    });
}

#[cfg(test)]
mod mock_backend {
    // 进程内共享的“凭据库”：键为 (service, user)，值为密文字节。
    // 模拟真实后端的跨 Entry 持久化语义，仅供测试使用。
    use keyring::credential::{CredentialApi, CredentialBuilderApi};
    use keyring::{Credential, Result};
    use std::any::Any;
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn store() -> &'static Mutex<HashMap<(String, String), Vec<u8>>> {
        static STORE: std::sync::OnceLock<Mutex<HashMap<(String, String), Vec<u8>>>> =
            std::sync::OnceLock::new();
        STORE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub(super) struct MapStoreBuilder;

    impl CredentialBuilderApi for MapStoreBuilder {
        fn build(
            &self,
            _target: Option<&str>,
            service: &str,
            user: &str,
        ) -> Result<Box<Credential>> {
            Ok(Box::new(MapStoreCredential {
                key: (service.to_string(), user.to_string()),
            }))
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    struct MapStoreCredential {
        key: (String, String),
    }

    impl CredentialApi for MapStoreCredential {
        fn set_secret(&self, secret: &[u8]) -> Result<()> {
            store()
                .lock()
                .expect("mock 凭据库锁中毒")
                .insert(self.key.clone(), secret.to_vec());
            Ok(())
        }

        fn get_secret(&self) -> Result<Vec<u8>> {
            match store().lock().expect("mock 凭据库锁中毒").get(&self.key) {
                Some(v) => Ok(v.clone()),
                None => Err(keyring::Error::NoEntry),
            }
        }

        fn delete_credential(&self) -> Result<()> {
            match store().lock().expect("mock 凭据库锁中毒").remove(&self.key) {
                Some(_) => Ok(()),
                None => Err(keyring::Error::NoEntry),
            }
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_support::temp_store;

    #[test]
    fn api_key_roundtrips_via_secret_store() {
        let _store = temp_store(); // 触发 mock 后端切换
        delete_api_key().unwrap(); // 清掉其他测试可能残留的条目，避免顺序依赖
        assert_eq!(get_api_key().unwrap(), None);
        put_api_key("k-1").unwrap();
        assert_eq!(get_api_key().unwrap().as_deref(), Some("k-1"));
        put_api_key("k-2").unwrap();
        assert_eq!(get_api_key().unwrap().as_deref(), Some("k-2"), "覆盖写");
        delete_api_key().unwrap();
        assert_eq!(get_api_key().unwrap(), None);
        delete_api_key().unwrap(); // 幂等
    }

    #[test]
    fn device_secret_roundtrips_via_secret_store() {
        let _store = temp_store();
        delete_device_secret().unwrap();
        assert_eq!(get_device_secret().unwrap(), None);
        put_device_secret("ab").unwrap();
        assert_eq!(get_device_secret().unwrap().as_deref(), Some("ab"));
        delete_device_secret().unwrap();
        assert_eq!(get_device_secret().unwrap(), None);
    }
}
