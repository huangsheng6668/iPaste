//! 云 API Key 的系统凭据库存取（Windows Credential Manager / macOS Keychain）。
//! 测试统一切换到进程内内存 mock（见 `use_mock_backend_for_tests`），
//! 不触碰真实系统库。

use keyring::Entry;

const SERVICE: &str = "iPaste";
const ACCOUNT: &str = "cloud_api_key";

fn entry() -> Result<Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| format!("无法访问系统凭据库：{e}"))
}

pub(crate) fn put_api_key(value: &str) -> Result<(), String> {
    entry()?.set_password(value).map_err(|e| format!("写入系统凭据库失败：{e}"))
}

// 读取路径由 Task 9（cloud_settings_with_conn 改造）接入，暂无生产调用方。
#[allow(dead_code)]
pub(crate) fn get_api_key() -> Result<Option<String>, String> {
    match entry()?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("读取系统凭据库失败：{e}")),
    }
}

/// 幂等删除：条目不存在视为成功（如从未配置过云同步）。
pub(crate) fn delete_api_key() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除系统凭据库条目失败：{e}")),
    }
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
            match store()
                .lock()
                .expect("mock 凭据库锁中毒")
                .remove(&self.key)
            {
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
}
