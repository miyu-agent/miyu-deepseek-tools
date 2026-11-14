//! DeepSeek 的 key 从哪来：全在 [`credentials_from`] 这一处，别处一律经它拿。key 只放在请求头里，报错、日志里都不写它
//! （[`Credentials`] 的 `Debug` 也不印）。

use std::fmt;

use serde_json::{Map, Value};

/// DeepSeek 接口的地址：没给测试地址时用它。
pub const DEFAULT_BASE: &str = "https://api.deepseek.com";
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
/// 核心交包用人的 DeepSeek 供应商 key 的那一项能力（PK-1）合进来以后，这里改成从握手、推送来的 `config` 里读那一格；
/// 现在那一格的名字还没定，只读两个测试钩子：`MIYU_DEEPSEEK_TEST_KEY`（key）、`MIYU_DEEPSEEK_TEST_BASE_URL`（接口地址，
/// 不写是 <https://api.deepseek.com>）。没有 key 的交回空的：合成、识别不登记，OCR 回「还没有 key」。
pub fn credentials_from(
    _config: &Map<String, Value>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Option<Credentials> {
    let key = env(TEST_KEY)?.trim().to_string();
    if key.is_empty() {
        return None;
    }
    let base = env(TEST_BASE_URL)
        .map(|base| base.trim().trim_end_matches('/').to_string())
        .filter(|base| !base.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE.to_string());
    Some(Credentials { key, base })
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::{Credentials, DEFAULT_BASE, TEST_BASE_URL, TEST_KEY, credentials_from};

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
