//! 和原来的 Python 脚本对答案：`tests/fixtures/status/` 里，`page.html` 是 2026-11-14 抓的一份真的状态页，`synthetic.html`
//! 是专门凑的各种边角（浮点可用率、真假当可用率、数字编号、时刻一样的、取整一半的、跨年、闰日、坏掉的段……），
//! `golden/*.out` 是原来的 `deepseek_status.py` 在同一时刻（monkeypatch 了 `fetch`、`datetime.now`）印出来的，
//! `cases.json` 列着每一份的参数和退出码。这里照同样的页、参数、时刻跑 Rust 的，要一字不差。

use std::path::PathBuf;

use serde_json::{Value, json};

use super::py::Moment;
use super::{DESCRIPTION, URL, collected, fetch, run, schema};
use crate::fake_http::{Answer, FakeDeepSeek};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/status")
}

fn read(name: &str) -> String {
    String::from_utf8(std::fs::read(fixtures().join(name)).unwrap()).unwrap()
}

#[test]
fn every_golden_output_matches_the_python_script_byte_for_byte() {
    let cases: Value = serde_json::from_str(&read("cases.json")).unwrap();
    let micros = cases["now_micros"].as_i64().unwrap();
    let now = Moment {
        seconds: micros / 1_000_000,
        micros: micros % 1_000_000,
    };
    let list = cases["cases"].as_array().unwrap();
    assert_eq!(list.len(), 29);
    for case in list {
        let name = case["name"].as_str().unwrap();
        let page = case["page"].as_str().unwrap();
        let outcome = if let Some(status) = page.strip_prefix('@') {
            // 状态页回错误码：真的经 HTTP 去取，假的服务回这个码。
            let server = FakeDeepSeek::start(vec![Answer::new(
                status.parse().unwrap(),
                "text/html",
                "busy",
            )]);
            let url = format!("{}/", server.base);
            run(&case["args"], || fetch(&url), now)
        } else {
            let html = read(page);
            run(&case["args"], || Ok(html), now)
        };
        let golden = read(&format!("golden/{name}.out"));
        assert_eq!(outcome.text, golden, "{name}");
        assert_eq!(outcome.ok, case["exit"] == 0, "{name}");
    }
}

#[test]
fn the_real_page_has_the_four_parts() {
    let found = collected(&read("page.html"));
    for key in [
        "initialPageConfig",
        "active_changes",
        "initialCalendarData",
        "component_uptimes",
    ] {
        assert!(found.contains_key(key), "{key}");
    }
    assert!(
        !found["initialPageConfig"]["components"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn fetch_sends_the_browser_headers_and_reads_the_page() {
    let server = FakeDeepSeek::start(vec![Answer::new(200, "text/html", "<html>ok</html>")]);
    let page = fetch(&format!("{}/", server.base)).unwrap();
    assert_eq!(page, "<html>ok</html>");
    let request = &server.received()[0];
    assert_eq!(
        (request.method.as_str(), request.path.as_str()),
        ("GET", "/")
    );
    assert!(
        request
            .header("user-agent")
            .unwrap()
            .starts_with("Mozilla/5.0")
    );
    assert_eq!(request.header("accept"), Some("text/html"));
    assert!(request.header("authorization").is_none(), "不用 key");
}

#[test]
fn an_unreachable_page_says_it_cannot_connect() {
    // 一个刚关掉的口：连不上。
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let why = fetch(&format!("http://127.0.0.1:{port}/")).unwrap_err();
    assert!(why.starts_with(&format!("连不上 {URL}：")), "{why}");
}

#[test]
fn the_schema_and_description_are_the_script_s() {
    assert_eq!(
        DESCRIPTION,
        "Check DeepSeek's official status page: each service (API models, chat, search, file upload) with its state and 30-day uptime, plus ongoing and recent incidents."
    );
    assert_eq!(schema()["properties"]["incidents"]["maximum"], 20);
    assert_eq!(
        schema()["properties"]["format"]["enum"],
        json!(["md", "json"])
    );
}
