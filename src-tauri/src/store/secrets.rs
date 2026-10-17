//! 云 API Key 的系统凭据库存取（Windows Credential Manager / macOS Keychain）。
//! 测试统一切换到进程内内存 mock（见 `use_mock_backend_for_tests`），
//! 不触碰真实系统库。

use std::sync::{Mutex, OnceLock};

use keyring::Entry;

const SERVICE: &str = "iPaste";
const ACCOUNT: &str = "cloud_api_key";
const ACCOUNT_DEVICE: &str = "device_sync_secret";
const ACCOUNT_OPENAI_OCR: &str = "openai_ocr_api_key";

/// 单个凭据的进程内缓存槽：`None` = 尚未加载（需穿透到真实凭据库），
/// `Some(None)` = 已确认未配置，`Some(Some(v))` = 已缓存值。
type SecretCacheSlot = OnceLock<Mutex<Option<Option<String>>>>;

static API_KEY_CACHE: SecretCacheSlot = OnceLock::new();
static DEVICE_SECRET_CACHE: SecretCacheSlot = OnceLock::new();
static OPENAI_OCR_KEY_CACHE: SecretCacheSlot = OnceLock::new();

/// 凭据读取的进程内缓存：keyring 访问是一次系统 IPC（macOS Keychain 走
/// securityd XPC），而设置读取出现在面板唤出、OCR 识别、托盘菜单等热路径，
/// 每次都打两遍凭据库并不划算。
///
/// 一致性前提：本进程是这些凭据的唯一写入方，且全部写入/删除路径都会同步
/// 更新缓存（见下方各 put/delete）。读取失败不缓存，下次仍会穿透重试；
/// 锁中毒时静默绕过缓存，行为退化为直接读凭据库。
fn cached_read(
    slot: &'static SecretCacheSlot,
    load: impl FnOnce() -> Result<Option<String>, String>,
) -> Result<Option<String>, String> {
    let guard = slot.get_or_init(|| Mutex::new(None));
    if let Ok(cache) = guard.lock() {
        if let Some(value) = cache.as_ref() {
            return Ok(value.clone());
        }
    }

    let loaded = load()?;
    if let Ok(mut cache) = guard.lock() {
        *cache = Some(loaded.clone());
    }
    Ok(loaded)
}

/// 写入成功后同步缓存（put 与 delete 共用：delete 传 `None`）。
fn cached_write(slot: &'static SecretCacheSlot, value: Option<String>) {
    let guard = slot.get_or_init(|| Mutex::new(None));
    if let Ok(mut cache) = guard.lock() {
        *cache = Some(value);
    }
}

fn entry() -> Result<Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| format!("无法访问系统凭据库：{e}"))
}

pub(crate) fn put_api_key(value: &str) -> Result<(), String> {
    entry()?
        .set_password(value)
        .map_err(|e| format!("写入系统凭据库失败：{e}"))?;
    cached_write(&API_KEY_CACHE, Some(value.to_string()));
    Ok(())
}

pub(crate) fn get_api_key() -> Result<Option<String>, String> {
    cached_read(&API_KEY_CACHE, || match entry()?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("读取系统凭据库失败：{e}")),
    })
}

/// 幂等删除：条目不存在视为成功（如从未配置过云同步）。
pub(crate) fn delete_api_key() -> Result<(), String> {
    let result = match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除系统凭据库条目失败：{e}")),
    };
    // 只有删除确实成功（或本就不存在）才把缓存归位为「未配置」，
    // 删除失败时保留旧值，避免缓存与凭据库失真。
    if result.is_ok() {
        cached_write(&API_KEY_CACHE, None);
    }
    result
}

fn device_entry() -> Result<Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT_DEVICE).map_err(|e| format!("无法访问系统凭据库：{e}"))
}

pub(crate) fn put_device_secret(value: &str) -> Result<(), String> {
    device_entry()?
        .set_password(value)
        .map_err(|e| format!("写入系统凭据库失败：{e}"))?;
    cached_write(&DEVICE_SECRET_CACHE, Some(value.to_string()));
    Ok(())
}

pub(crate) fn get_device_secret() -> Result<Option<String>, String> {
    cached_read(&DEVICE_SECRET_CACHE, || match device_entry()?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("读取系统凭据库失败：{e}")),
    })
}

/// 测试清理用；生产代码不调用（设备身份条目跟随 app 卸载清除）。
#[cfg(test)]
pub(crate) fn delete_device_secret() -> Result<(), String> {
    let result = match device_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除系统凭据库条目失败：{e}")),
    };
    if result.is_ok() {
        cached_write(&DEVICE_SECRET_CACHE, None);
    }
    result
}

/// 清理已移除的智谱专用 OCR 引擎（≤0.9.9）遗留的凭据库条目。
/// 幂等、尽力而为：条目不存在或凭据库不可用都直接忽略。
pub(crate) fn remove_legacy_bigmodel_api_key() {
    if let Ok(entry) = keyring::Entry::new(SERVICE, "bigmodel_api_key") {
        let _ = entry.delete_credential();
    }
}

fn openai_ocr_entry() -> Result<Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT_OPENAI_OCR).map_err(|e| format!("无法访问系统凭据库：{e}"))
}

/// 通用 OpenAI 兼容接口的 API Key（与智谱 Key 分账户，切换引擎互不影响）。
pub(crate) fn put_openai_ocr_api_key(value: &str) -> Result<(), String> {
    openai_ocr_entry()?
        .set_password(value)
        .map_err(|e| format!("写入系统凭据库失败：{e}"))?;
    cached_write(&OPENAI_OCR_KEY_CACHE, Some(value.to_string()));
    Ok(())
}

pub(crate) fn get_openai_ocr_api_key() -> Result<Option<String>, String> {
    cached_read(&OPENAI_OCR_KEY_CACHE, || match openai_ocr_entry()?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("读取系统凭据库失败：{e}")),
    })
}

/// 幂等删除（从未配置过视为成功）。
pub(crate) fn delete_openai_ocr_api_key() -> Result<(), String> {
    let result = match openai_ocr_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除系统凭据库条目失败：{e}")),
    };
    if result.is_ok() {
        cached_write(&OPENAI_OCR_KEY_CACHE, None);
    }
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
