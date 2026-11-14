//! 工具 `deepseek_balance`：查 DeepSeek 账户的余额（`GET /user/balance`，DeepSeek 文档 Get User Balance）。回答是
//! `{"is_available", "balance_infos": [{"currency", "total_balance", "granted_balance", "topped_up_balance"}]}`，几个余额是字
//! （`"110.00"`）。给模型看：账户能不能调接口，每种货币的总余额、赠送的、充值的；原样的回答放 `data`。

use serde_json::Value;

use crate::http::{Api, DeepSeek, said};

pub const DESCRIPTION: &str = "Check the balance of the DeepSeek account: whether it can still make API calls, and the total, granted and topped-up balance per currency.";

/// 没有 key 时回的：什么都没发。
pub const NO_KEY: &str = "No DeepSeek key yet, so the balance was not checked.";

/// 查一次：交回给模型看的字和 DeepSeek 原样的回答。
///
/// # Errors
///
/// 连不上、DeepSeek 报错（[`DeepSeek::get`]）、回答不是 JSON 或带着 `error`：一句英文，不带 key。
pub fn check(deepseek: &DeepSeek) -> Result<(String, Value), String> {
    let answer = deepseek.get(Api::Balance)?;
    let json: Value = serde_json::from_slice(&answer)
        .map_err(|_| "DeepSeek balance check sent an answer that is not JSON.".to_string())?;
    if !json["error"].is_null() {
        let why = said(&answer).unwrap_or_else(|| "no reason given".to_string());
        return Err(deepseek.scrub(&format!("DeepSeek balance check failed: {why}")));
    }
    Ok((shown(&json), json))
}

/// 给模型看的：头一行能不能调接口，下面一种货币一行。
pub fn shown(answer: &Value) -> String {
    let mut lines = vec![
        match answer["is_available"].as_bool() {
            Some(true) => "The DeepSeek account can make API calls.",
            Some(false) => "The DeepSeek account cannot make API calls: the balance is not enough.",
            None => "DeepSeek did not say whether the account can make API calls.",
        }
        .to_string(),
    ];
    let infos: Vec<&Value> = answer["balance_infos"]
        .as_array()
        .map(|infos| infos.iter().collect())
        .unwrap_or_default();
    if infos.is_empty() {
        lines.push("DeepSeek listed no balance.".to_string());
    }
    for info in infos {
        let field = |name: &str| match &info[name] {
            Value::String(text) => text.clone(),
            Value::Number(number) => number.to_string(),
            _ => "?".to_string(),
        };
        lines.push(format!(
            "- {}: total {}, granted {}, topped up {}",
            field("currency"),
            field("total_balance"),
            field("granted_balance"),
            field("topped_up_balance")
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::shown;

    #[test]
    fn one_line_per_currency_and_missing_parts_said_plainly() {
        let answer = json!({"is_available": true, "balance_infos": [
            {"currency": "CNY", "total_balance": "110.00", "granted_balance": "10.00", "topped_up_balance": "100.00"},
            {"currency": "USD", "total_balance": 1.5}
        ]});
        assert_eq!(
            shown(&answer),
            "The DeepSeek account can make API calls.\n\
- CNY: total 110.00, granted 10.00, topped up 100.00\n\
- USD: total 1.5, granted ?, topped up ?"
        );
        assert_eq!(
            shown(&json!({})),
            "DeepSeek did not say whether the account can make API calls.\nDeepSeek listed no balance."
        );
    }
}
