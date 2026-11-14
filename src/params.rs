//! 方法参数的检查（核心 `docs/reference/bus.md`「公认的名字」：不认识的字段报 `bad_params`，写错名字的一方早知道）。

use miyu_ext::MethodError;
use serde_json::Value;

/// 参数里只能有这几格；不是对象的（`null`、不写）当空的。
pub fn only(params: &Value, allowed: &[&str]) -> Result<(), MethodError> {
    match params {
        Value::Null => Ok(()),
        Value::Object(fields) => match fields.keys().find(|key| !allowed.contains(&key.as_str())) {
            Some(unknown) => Err(bad(&format!("Unknown field {unknown}."))),
            None => Ok(()),
        },
        _ => Err(bad("The params must be an object.")),
    }
}

/// 一格必写的字。
pub fn text<'a>(params: &'a Value, name: &str) -> Result<&'a str, MethodError> {
    params[name]
        .as_str()
        .ok_or_else(|| bad(&format!("Give {name} as a string.")))
}

pub fn bad(message: &str) -> MethodError {
    MethodError::new("bad_params", message)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{only, text};

    #[test]
    fn unknown_fields_and_wrong_types_are_bad_params() {
        assert!(only(&json!({"text": "x"}), &["text", "voice"]).is_ok());
        assert!(only(&json!(null), &[]).is_ok());
        let error = only(&json!({"txt": "x"}), &["text"]).unwrap_err();
        assert_eq!(error.code.as_deref(), Some("bad_params"));
        assert_eq!(error.message, "Unknown field txt.");
        assert!(only(&json!([1]), &[]).is_err());
        assert_eq!(text(&json!({"a": "b"}), "a").unwrap(), "b");
        assert_eq!(
            text(&json!({"a": 1}), "a").unwrap_err().message,
            "Give a as a string."
        );
    }
}
