//! 工具 `deepseek_status`：查 DeepSeek 的官方状态页 <https://status.deepseek.com/>，各项服务现在好不好、近 30 天可用率，
//! 正在处理的和最近的故障。照 miyu-scripts 的 `deepseek_status.py` 搬成 Rust（Windows 上不一定有 Python），参数、说明、
//! 输出一字不差（`tests/fixtures/status/` 里是原来的脚本对同一张页、同一时刻的输出）；Python 的规矩见 [`py`]。
//!
//! 状态页是 Next.js 的页面，数据在一段段 `self.__next_f.push([1,"…"])` 里：取 `initialPageConfig`（服务列表）、
//! `component_uptimes`（可用率）、`active_changes`（正在处理的）、`initialCalendarData`（历史）。不用 key。

mod py;
mod render;

use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

use serde_json::{Value, json};

use self::py::{Moment, get, str_of};

/// 状态页。
pub const URL: &str = "https://status.deepseek.com/";
/// 照原来的脚本发的浏览器标识。
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";
const MARKER: &str = "self.__next_f.push([1,\"";
const KEYS: [&str; 4] = [
    "initialPageConfig",
    "active_changes",
    "initialCalendarData",
    "component_uptimes",
];
/// 认得的参数（原来脚本开头 Parameters 里的）。
const KNOWN: [&str; 2] = ["format", "incidents"];
/// 页面最多读多少：现在约 220 KB。
const LONGEST: u64 = 16 * 1024 * 1024;

/// 给模型看的说明：原来脚本的 `# Description:` 一行。
pub const DESCRIPTION: &str = "Check DeepSeek's official status page: each service (API models, chat, search, file upload) with its state and 30-day uptime, plus ongoing and recent incidents.";

/// 参数的格式：原来脚本的 Parameters。
pub fn schema() -> Value {
    json!({"type": "object", "properties": {
        "incidents": {"type": "integer", "minimum": 0, "maximum": 20, "description": "How many recent incidents to list, newest first; default 3, 0 for none."},
        "format": {"type": "string", "enum": ["md", "json"], "description": "md (default) for reading; json for code, e.g. piping miyu call into jq or python."}
    }})
}

/// 一次查询的结果：原来脚本印到标准输出的字（带末尾的换行），和它是不是成功退出的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub text: String,
    pub ok: bool,
}

/// 照参数查一次：`fetch` 取页面（出错交回要说的那一句），`now` 是现在。
pub fn run(args: &Value, fetch: impl FnOnce() -> Result<String, String>, now: Moment) -> Outcome {
    let Some(fields) = args.as_object() else {
        return fail(false, "参数要是一个 JSON 对象");
    };
    let as_json = get(args, "format").as_str() == Some("json");
    let mut unknown: Vec<&str> = fields
        .keys()
        .map(String::as_str)
        .filter(|key| !KNOWN.contains(key))
        .collect();
    if !unknown.is_empty() {
        unknown.sort_unstable();
        return fail(
            as_json,
            &format!(
                "不认识的参数：{}。能用的：{}",
                unknown.join("、"),
                KNOWN.join("、")
            ),
        );
    }
    let format = get(args, "format");
    if !(format.is_null() || matches!(format.as_str(), Some("md" | "json"))) {
        return fail(
            as_json,
            &format!("format 只认 md、json，不认 {}", str_of(format)),
        );
    }
    let count = match fields.get("incidents") {
        None => 3,
        Some(Value::Number(number)) if number.is_i64() || number.is_u64() => {
            number.as_i64().map_or(20, |count| count.clamp(0, 20))
        }
        Some(_) => return fail(as_json, "incidents 要是 0 到 20 的整数"),
    };
    let html = match fetch() {
        Ok(html) => html,
        Err(why) => return fail(as_json, &why),
    };
    let found = collected(&html);
    match render::page(&found, usize::try_from(count).unwrap_or(0), now, as_json) {
        Ok(text) => Outcome {
            text: format!("{text}\n"),
            ok: true,
        },
        Err(why) => fail(as_json, &why),
    }
}

/// 工具被调到时：现在去取真的状态页。
pub fn answer(args: &Value) -> Outcome {
    run(args, || fetch(URL), Moment::now())
}

/// 出错：md 的写 `错误：…`，json 的写 `{"ok": false, "error": …}`。
fn fail(as_json: bool, message: &str) -> Outcome {
    let text = if as_json {
        py::dumps(&json!({"ok": false, "error": message}))
    } else {
        format!("错误：{message}")
    };
    Outcome {
        text: format!("{text}\n"),
        ok: false,
    }
}

/// 取页面：GET，带浏览器标识，至多等 20 秒，不是 2xx 的算失败。走系统代理（[`crate::http::agent_for`]）。
///
/// # Errors
///
/// 对方回的不是 2xx、连不上：照原来脚本的说法。
pub fn fetch(url: &str) -> Result<String, String> {
    let agent = crate::http::agent_for(url);
    let response = agent
        .get(url)
        .header("User-Agent", UA)
        .header("Accept", "text/html")
        .config()
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .call()
        .map_err(|error| format!("连不上 {URL}：{error}"))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!(
            "状态页回 HTTP {status}。可以让用户自己打开 {URL} 看"
        ));
    }
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(LONGEST)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("连不上 {URL}：{error}"))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// 从页面里取那四样：每一样取第一次碰到的（深度优先，字典照原来的先后）。
pub fn collected(html: &str) -> HashMap<&'static str, Value> {
    let mut found = HashMap::new();
    for data in chunks(html) {
        collect(&data, &mut found);
    }
    found
}

/// 每段 push 的内容是一个 JS 的字，照 JSON 解开；里面一行行是 `<编号>:<JSON>`，解得开的交出来。
fn chunks(html: &str) -> Vec<Value> {
    let bytes = html.as_bytes();
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(found) = html.get(pos..).and_then(|rest| rest.find(MARKER)) {
        let start = pos + found + MARKER.len();
        let mut end = start;
        while end < bytes.len() {
            if bytes[end] == b'\\' {
                // 跳过反斜杠和它后面那一个字（照字跳，不是照字节）。
                end += 1 + html[end + 1..].chars().next().map_or(0, char::len_utf8);
                continue;
            }
            if bytes[end] == b'"' && html[end + 1..].starts_with("])") {
                break;
            }
            end += 1;
        }
        let end = end.min(bytes.len());
        pos = end;
        let Ok(text) = serde_json::from_str::<String>(&format!("\"{}\"", &html[start..end])) else {
            continue;
        };
        for line in text.split('\n') {
            if let Some((_, body)) = line.split_once(':')
                && let Ok(value) = serde_json::from_str::<Value>(body)
            {
                out.push(value);
            }
        }
    }
    out
}

fn collect(value: &Value, found: &mut HashMap<&'static str, Value>) {
    match value {
        Value::Object(fields) => {
            for key in KEYS {
                if !found.contains_key(key)
                    && let Some(value) = fields.get(key)
                {
                    found.insert(key, value.clone());
                }
            }
            for child in fields.values() {
                collect(child, found);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect(item, found);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
