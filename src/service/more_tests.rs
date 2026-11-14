//! 工具 `deepseek_balance` 经假 DeepSeek 跑一遍，和三件工具的规格、清单对得上。

use serde_json::{Value, json};

use super::harness::{KEY, reply_text, start, start_with_key};
use super::{BALANCE_TOOL, STATUS_TOOL, TOOL};
use crate::balance::NO_KEY;
use crate::fake_http::{Answer, FakeDeepSeek};

fn spec(core: &miyu_ext::FakeCore, name: &str) -> Value {
    core.tools()
        .into_iter()
        .find(|tool| tool["name"] == name)
        .unwrap_or_else(|| panic!("{name} is registered"))
}

#[test]
fn balance_asks_get_user_balance_with_the_key_and_shows_one_currency() {
    let answer = json!({"is_available": true, "balance_infos": [
        {"currency": "CNY", "total_balance": "110.00", "granted_balance": "10.00", "topped_up_balance": "100.00"}]});
    let server = FakeDeepSeek::start(vec![Answer::new(
        200,
        "application/json",
        answer.to_string(),
    )]);
    let (core, _) = start_with_key(&server.base);
    let reply = core.call_tool(BALANCE_TOOL, json!({}));
    assert!(reply.get("error").is_none(), "{reply}");
    assert_eq!(
        reply_text(&reply),
        "The DeepSeek account can make API calls.\n- CNY: total 110.00, granted 10.00, topped up 100.00"
    );
    assert_eq!(reply["data"], answer, "原样的回答给脚本");
    let received = server.received();
    assert_eq!(received.len(), 1);
    assert_eq!(
        (received[0].method.as_str(), received[0].path.as_str()),
        ("GET", "/user/balance")
    );
    let bearer = format!("Bearer {KEY}");
    assert_eq!(received[0].header("authorization"), Some(bearer.as_str()));
    core.close().unwrap();
}

#[test]
fn balance_with_two_currencies_and_not_enough_money() {
    let answer = json!({"is_available": false, "balance_infos": [
        {"currency": "CNY", "total_balance": "0.00", "granted_balance": "0.00", "topped_up_balance": "0.00"},
        {"currency": "USD", "total_balance": "0.50", "granted_balance": "0.50", "topped_up_balance": "0.00"}]});
    let server = FakeDeepSeek::start(vec![Answer::new(
        200,
        "application/json",
        answer.to_string(),
    )]);
    let (core, _) = start_with_key(&server.base);
    let reply = core.call_tool(BALANCE_TOOL, json!({}));
    assert_eq!(
        reply_text(&reply),
        "The DeepSeek account cannot make API calls: the balance is not enough.\n\
- CNY: total 0.00, granted 0.00, topped up 0.00\n\
- USD: total 0.50, granted 0.50, topped up 0.00"
    );
    core.close().unwrap();
}

#[test]
fn balance_without_a_key_sends_nothing() {
    let (core, _) = start(Box::new(|_| None));
    let reply = core.call_tool(BALANCE_TOOL, json!({}));
    assert_eq!(reply["error"], true);
    assert_eq!(reply_text(&reply), NO_KEY);
    core.close().unwrap();
}

#[test]
fn a_refused_key_is_401_without_the_key() {
    let refused = format!(
        r#"{{"error": {{"message": "Authentication Fails, Your api key: {KEY} is invalid"}}}}"#
    );
    let server = FakeDeepSeek::start(vec![Answer::new(401, "application/json", refused)]);
    let (core, _) = start_with_key(&server.base);
    let reply = core.call_tool(BALANCE_TOOL, json!({}));
    assert_eq!(reply["error"], true);
    let text = reply_text(&reply);
    assert!(
        text.starts_with("DeepSeek balance check failed (401: DeepSeek refused the key)"),
        "{text}"
    );
    assert!(
        text.contains("<key>") && !reply.to_string().contains(KEY),
        "{reply}"
    );
    core.close().unwrap();
}

#[test]
fn balance_takes_no_parameters() {
    let server = FakeDeepSeek::start(Vec::new());
    let (core, _) = start_with_key(&server.base);
    let reply = core.call_tool(BALANCE_TOOL, json!({"currency": "CNY"}));
    assert_eq!(reply["error"], true);
    assert_eq!(
        reply_text(&reply),
        "Unknown parameter currency: deepseek_balance takes no parameters."
    );
    assert!(server.received().is_empty());
    core.close().unwrap();
}

#[test]
fn the_three_tools_are_declared_as_the_manifest_feature_lists_them() {
    let (core, _) = start(Box::new(|_| None));
    let status = spec(&core, STATUS_TOOL);
    assert_eq!(status["access"], "network");
    assert_eq!(status["clicall"], true);
    assert_eq!(status["timeout_ms"], 30_000);
    assert_eq!(
        status["display_name"],
        json!({"en": "DeepSeek status", "zh": "DeepSeek 状态查询"})
    );
    let balance = spec(&core, BALANCE_TOOL);
    assert_eq!(balance["access"], "network");
    assert_eq!(
        balance["display_name"],
        json!({"en": "DeepSeek balance", "zh": "DeepSeek 余额查询"})
    );
    let ocr = spec(&core, TOOL);
    assert_eq!(ocr["access"], "outbound");
    assert_eq!(
        ocr["display_name"],
        json!({"en": "DeepSeek OCR", "zh": "DeepSeek 文字识别"})
    );
    // 清单 [features.deepseek-tools] 的 tools 要正是登记的这三件，照这个先后。
    let manifest = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("package/deepseek-tools/package.toml"),
    )
    .unwrap();
    let listed = manifest
        .lines()
        .skip_while(|line| *line != "[features.deepseek-tools]")
        .find(|line| line.starts_with("tools = "))
        .unwrap();
    assert_eq!(
        listed,
        r#"tools = ["deepseek_status", "deepseek_balance", "deepseek_ocr"]"#
    );
    let mut registered: Vec<String> = core
        .tools()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect();
    registered.sort();
    assert_eq!(
        registered,
        ["deepseek_balance", "deepseek_ocr", "deepseek_status"]
    );
    core.close().unwrap();
}
