//! DeepSeek 的 key 从哪来：全在 [`credentials_from`] 这一处，别处一律经它拿。key 只放在请求头里，报错、日志里都不写它
//! （[`Credentials`] 的 `Debug` 也不印）。

use std::fmt;

use serde_json::{Map, Value};

/// DeepSeek 接口的地址：人的 key 一律只发到这里；测试 key 没给测试地址时也用它。
pub const DEFAULT_BASE: &str = "https://api.deepseek.com";
/// 核心交来的那一格：人在 DeepSeek 供应商那里写的 key（PK-1，清单的 `[provider_key] catalog = "deepseek"`）。
pub const PROVIDER_KEY: &str = "provider_key";
/// 测试用的钩子：key。
pub const TEST_KEY: &str = "MIYU_DEEPSEEK_TEST_KEY";
/// 测试用的钩子：接口地址（指到本机假的服务上）。
pub const TEST_BASE_URL: &str = "MIYU_DEEPSEEK_TEST_BASE_URL";

/// 连 DeepSeek 要的：key 和接口地址（末尾不带 `/`）。
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub key: String,
    pub base: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("key", &"<hidden>")
            .field("base", &self.base)
            .finish()
    }
}

/// 现在有没有 key：环境变量照进程的读。
pub fn credentials(config: &Map<String, Value>) -> Option<Credentials> {
    credentials_from(config, &|name| std::env::var(name).ok())
}

/// 拿 key 的唯一一处。`env` 是读环境变量的办法：程序里读进程的，测试交假的（改进程的环境要 unsafe，仓库不许）。
///
/// 先后：
///
/// 1. 测试钩子 `MIYU_DEEPSEEK_TEST_KEY` 有值的用它；只有这时才认 `MIYU_DEEPSEEK_TEST_BASE_URL`（接口地址，不写是
///    <https://api.deepseek.com>），好指到本机假的服务上。
/// 2. 不然用核心在握手、`extension.config` 推送里交的 `provider_key`（人的 DeepSeek key，PK-1；推 `null` 是删了），地址
///    一律是 <https://api.deepseek.com>：环境变量改不了人的 key 发去哪儿。
///
/// 都没有的交回空的：合成、识别不登记，余额、OCR 回「还没有 key」。
pub fn credentials_from(
    config: &Map<String, Value>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Option<Credentials> {
    if let Some(key) = env(TEST_KEY).and_then(|key| filled(&key)) {
        let base = env(TEST_BASE_URL)
            .map(|base| base.trim().trim_end_matches('/').to_string())
            .filter(|base| !base.is_empty())
            .unwrap_or_else(|| DEFAULT_BASE.to_string());
        return Some(Credentials { key, base });
    }
    let key = config.get(PROVIDER_KEY)?.as_str().and_then(filled)?;
    Some(Credentials {
        key,
        base: DEFAULT_BASE.to_string(),
    })
}

/// 去掉两头空白；空的当没有。
fn filled(key: &str) -> Option<String> {
    let key = key.trim();
    (!key.is_empty()).then(|| key.to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};

    use super::{
        Credentials, DEFAULT_BASE, PROVIDER_KEY, TEST_BASE_URL, TEST_KEY, credentials_from,
    };

    #[test]
    fn no_key_means_none_and_the_base_has_a_default() {
        assert_eq!(credentials_from(&Map::new(), &|_| None), None);
        assert_eq!(
            credentials_from(&Map::new(), &|name| (name == TEST_KEY).then(|| "  ".into())),
            None
        );
        let found = credentials_from(&Map::new(), &|name| {
            (name == TEST_KEY).then(|| "sk-1".to_string())
        });
        assert_eq!(
            found,
            Some(Credentials {
                key: "sk-1".into(),
                base: DEFAULT_BASE.into()
            })
        );
        let local = credentials_from(&Map::new(), &|name| match name {
            TEST_KEY => Some("sk-1".into()),
            TEST_BASE_URL => Some("http://127.0.0.1:9/".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(local.base, "http://127.0.0.1:9");
    }

    fn with_key(key: Value) -> Map<String, Value> {
        let mut config = Map::new();
        config.insert(PROVIDER_KEY.into(), key);
        config
    }

    #[test]
    fn the_provider_key_from_the_core_always_goes_to_deepseek() {
        let config = with_key(json!(" sk-real "));
        assert_eq!(
            credentials_from(&config, &|_| None),
            Some(Credentials {
                key: "sk-real".into(),
                base: DEFAULT_BASE.into()
            })
        );
        // 测试地址只跟测试 key 走：只设了地址的，人的 key 照旧只发 DeepSeek。
        let moved = credentials_from(&config, &|name| {
            (name == TEST_BASE_URL).then(|| "http://127.0.0.1:9".to_string())
        })
        .unwrap();
        assert_eq!(moved.key, "sk-real");
        assert_eq!(moved.base, DEFAULT_BASE);
        // 设了测试 key 的用测试 key：人的 key 不会跟着测试地址出去。
        let test = credentials_from(&config, &|name| match name {
            TEST_KEY => Some("sk-test".into()),
            TEST_BASE_URL => Some("http://127.0.0.1:9".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(test.key, "sk-test");
        assert_eq!(test.base, "http://127.0.0.1:9");
    }

    #[test]
    fn an_empty_null_or_odd_provider_key_is_no_key() {
        for odd in [
            json!(""),
            json!("  "),
            json!(null),
            json!(1),
            json!({"key": "sk"}),
        ] {
            assert_eq!(
                credentials_from(&with_key(odd.clone()), &|_| None),
                None,
                "{odd}"
            );
        }
    }

    #[test]
    fn debug_never_shows_the_key() {
        let shown = format!(
            "{:?}",
            Credentials {
                key: "sk-secret".into(),
                base: DEFAULT_BASE.into()
            }
        );
        assert!(!shown.contains("sk-secret"), "{shown}");
    }
}
