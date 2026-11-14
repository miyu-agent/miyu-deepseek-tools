//! 测试的架子：起扩展、假核心（开发包的 `FakeCore`，替核心存 blob）、找 key 的办法指到假 DeepSeek 上。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use miyu_ext::{Extension, FakeCore};
use serde_json::{Map, Value, json};

use super::{Lookup, Service};
use crate::credentials::{TEST_BASE_URL, TEST_KEY, credentials_from};

/// 测试用的 key：哪条报错里都不该出现。
pub const KEY: &str = "sk-test-5f3e9a1c0b7d";

/// 经测试钩子（`MIYU_DEEPSEEK_TEST_KEY`、`MIYU_DEEPSEEK_TEST_BASE_URL`）找 key，两个钩子的值是 [`KEY`] 和 `base`。
pub fn hooks_pointing_at(base: &str) -> Lookup {
    let base = base.to_string();
    Box::new(move |config| {
        credentials_from(config, &|name| match name {
            TEST_KEY => Some(KEY.to_string()),
            TEST_BASE_URL => Some(base.clone()),
            _ => None,
        })
    })
}

/// 假核心替扩展存的 blob。
#[derive(Clone, Default)]
pub struct Blobs {
    stored: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    uploads: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    next: Arc<AtomicU64>,
}

impl Blobs {
    /// 存一份，交回编号。
    pub fn add(&self, bytes: Vec<u8>) -> String {
        let id = format!("sha256:fake{}", self.next.fetch_add(1, Ordering::Relaxed));
        self.stored.lock().unwrap().insert(id.clone(), bytes);
        id
    }

    pub fn get(&self, id: &str) -> Vec<u8> {
        self.stored.lock().unwrap()[id].clone()
    }

    /// 让假核心答 `blob.put`、`blob.open/write/close`、`blob.get`。
    fn answer(&self, core: &FakeCore) {
        let blobs = self.clone();
        core.answer("blob.put", move |params| {
            let bytes = STANDARD.decode(params["data"].as_str().unwrap()).unwrap();
            Ok(json!({"blob": blobs.add(bytes)}))
        });
        let blobs = self.clone();
        core.answer("blob.open", move |_| {
            let upload = format!("u{}", blobs.next.fetch_add(1, Ordering::Relaxed));
            blobs
                .uploads
                .lock()
                .unwrap()
                .insert(upload.clone(), Vec::new());
            Ok(json!({"upload": upload}))
        });
        let blobs = self.clone();
        core.answer("blob.write", move |params| {
            let data = STANDARD.decode(params["data"].as_str().unwrap()).unwrap();
            let mut uploads = blobs.uploads.lock().unwrap();
            let open = uploads.get_mut(params["upload"].as_str().unwrap()).unwrap();
            assert_eq!(params["offset"].as_u64().unwrap(), open.len() as u64);
            open.extend(data);
            Ok(json!({"received": open.len()}))
        });
        let blobs = self.clone();
        core.answer("blob.close", move |params| {
            let bytes = blobs
                .uploads
                .lock()
                .unwrap()
                .remove(params["upload"].as_str().unwrap())
                .unwrap();
            Ok(json!({"blob": blobs.add(bytes)}))
        });
        let blobs = self.clone();
        core.answer("blob.get", move |params| {
            let bytes = blobs.get(params["blob"].as_str().unwrap());
            let offset = usize::try_from(params["offset"].as_u64().unwrap_or(0)).unwrap();
            let length = usize::try_from(params["length"].as_u64().unwrap()).unwrap();
            let end = (offset + length).min(bytes.len());
            Ok(json!({"data": STANDARD.encode(&bytes[offset.min(end)..end]), "size": bytes.len()}))
        });
    }
}

/// 起扩展，交回假核心和它存的 blob。
pub fn start(lookup: Lookup) -> (FakeCore, Blobs) {
    let service = Service::new(lookup);
    let mut ext = Extension::new("miyu-deepseek-tools", "0");
    service.register(&mut ext);
    let core = FakeCore::start(ext, Map::new());
    let blobs = Blobs::default();
    blobs.answer(&core);
    (core, blobs)
}

/// 起扩展，key 经测试钩子指到 `base`；等握手以后的那次重找 key 做完（插口方法登记上了）再交回。
pub fn start_with_key(base: &str) -> (FakeCore, Blobs) {
    let (core, blobs) = start(hooks_pointing_at(base));
    for method in super::METHODS {
        wait_for(&core, method, true);
    }
    (core, blobs)
}

/// 登记了的方法名。
pub fn registered(core: &FakeCore) -> Vec<String> {
    core.methods()
        .iter()
        .filter_map(|method| method["name"].as_str().map(str::to_string))
        .collect()
}

/// 等登记的方法里有（`present`）或没有 `name`：握手以后的登记在扩展自己的线程上发。
pub fn wait_for(core: &FakeCore, name: &str, present: bool) {
    let start = Instant::now();
    while registered(core).iter().any(|one| one == name) != present {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{name} registered should be {present}: {:?}",
            registered(core)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 照核心那样调一个方法：结果，或 `{"error": …}`。
pub fn call(core: &FakeCore, method: &str, params: &Value) -> Value {
    core.request("method.call", json!({"method": method, "params": params}))
}

/// 工具回答里给模型看的字。
pub fn reply_text(reply: &Value) -> String {
    reply["blocks"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// 一个测试自己的临时目录，测完删掉。
pub struct TempDir(pub std::path::PathBuf);

impl TempDir {
    pub fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "miyu-deepseek-tools-test-{name}-{}",
            std::process::id()
        ));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}
