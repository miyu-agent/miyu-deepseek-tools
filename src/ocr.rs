//! OCR：一个文件（PDF、Word、PowerPoint、图片）传给 DeepSeek 的 `POST /v1/tools/ocr`（multipart：`purpose`、`file`），逐页
//! 拿回字。DeepSeek 照页数收钱，文件离开这台电脑。
//!
//! - 至多 100 MiB，传之前先看大小；文件边读边发，不整个读进内存。
//! - 认的时候 DeepSeek 先发一串换行保持连接，再发 JSON：解析前去掉开头的空白。
//! - 状态码 200 也可能是失败：回答里有 `error` 的当失败。
//! - 逐页认的字带着定位记号（`<｜ref｜>`、`<｜box｜>` 和坐标，[`clean()`]）：给模型看的去掉，原样的留在 `data` 里。

mod clean;

use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use serde_json::{Value, json};

use crate::http::{Body, DeepSeek, said};
use crate::multipart::{Form, boundary};

pub use clean::clean;

/// 至多多大：DeepSeek 文档的 100 MiB。
pub const LARGEST: u64 = 100 * 1024 * 1024;

/// 认法（DeepSeek 的 `purpose`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// `text_recognition`：没有图的 Word、PowerPoint 直接取字，别的逐页认。
    Text,
    /// `rich_layout_recognition`：什么文件都逐页认。
    Layout,
}

impl Purpose {
    /// 照参数认，没写的是 [`Purpose::Text`]。
    ///
    /// # Errors
    ///
    /// 不是那两个词。
    pub fn parse(value: &Value) -> Result<Purpose, String> {
        match value {
            Value::Null => Ok(Purpose::Text),
            Value::String(word) if word == "text_recognition" => Ok(Purpose::Text),
            Value::String(word) if word == "rich_layout_recognition" => Ok(Purpose::Layout),
            other => Err(format!(
                "Unknown purpose {other}: use text_recognition or rich_layout_recognition."
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Purpose::Text => "text_recognition",
            Purpose::Layout => "rich_layout_recognition",
        }
    }
}

/// 把 `path` 传给 DeepSeek 认，交回它的回答（JSON，原样）。
///
/// # Errors
///
/// 读不了、不是文件、是空的、超过 100 MiB（这几样没传）；连不上、DeepSeek 报错（[`DeepSeek::post`]、[`parse`]）。
pub fn recognize(deepseek: &DeepSeek, path: &Path, purpose: Purpose) -> Result<Value, String> {
    let shown = path.display();
    let meta = std::fs::metadata(path).map_err(|error| format!("Cannot read {shown}: {error}."))?;
    if !meta.is_file() {
        return Err(format!("{shown} is not a file."));
    }
    let size = meta.len();
    if size == 0 {
        return Err(format!("{shown} is empty."));
    }
    if size > LARGEST {
        return Err(format!(
            "{shown} is {:.1} MiB; DeepSeek OCR takes at most 100 MiB. Nothing was uploaded.",
            mib(size)
        ));
    }
    let file = File::open(path).map_err(|error| format!("Cannot read {shown}: {error}."))?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let form = Form::new(
        &boundary(),
        &[("purpose", purpose.as_str())],
        ("file", &name, "application/octet-stream"),
    );
    let length = form.length(size);
    let reader = Cursor::new(form.head.clone())
        .chain(file.take(size))
        .chain(Cursor::new(form.tail.clone()));
    let answer = deepseek.post(
        crate::http::Api::Ocr,
        &form.content_type,
        Body::Stream {
            reader: Box::new(reader),
            length,
        },
    )?;
    parse(&answer).map_err(|why| deepseek.scrub(&why))
}

/// 读 DeepSeek 的回答：去掉开头保持连接的换行，带 `error` 的当失败，要有 `pages`。
///
/// # Errors
///
/// 不是 JSON、带着 `error`、没有 `pages`：一句英文。
pub fn parse(answer: &[u8]) -> Result<Value, String> {
    let text = String::from_utf8_lossy(answer);
    let json: Value = serde_json::from_str(text.trim_start())
        .map_err(|_| "DeepSeek OCR sent an answer that is not JSON.".to_string())?;
    if !json["error"].is_null() {
        let why = said(answer).unwrap_or_else(|| "no reason given".to_string());
        return Err(format!("DeepSeek OCR failed: {why}"));
    }
    if !json["pages"].is_array() {
        return Err("DeepSeek OCR sent no pages.".to_string());
    }
    Ok(json)
}

/// 给人、给模型看的字：头一行说几页、DeepSeek 照页收钱，再一页页 `--- page N ---` 加去掉记号的字。超过 `budget` 字节
/// 的后面几页不放，写一句没放哪几页。
pub fn shown(result: &Value, budget: usize) -> String {
    let pages = result["pages"].as_array().map_or(&[][..], Vec::as_slice);
    let count = result["page_count"].as_u64().unwrap_or(pages.len() as u64);
    let unit = if count == 1 { "page" } else { "pages" };
    let mut out = format!("{count} {unit} recognized; DeepSeek bills OCR per page.\n");
    for (at, page) in pages.iter().enumerate() {
        let number = page["index"]
            .as_u64()
            .map_or(at as u64 + 1, |index| index + 1);
        let text = clean(page["text"].as_str().unwrap_or_default());
        let mut part = format!(
            "\n--- page {number} ---\n{}\n",
            if text.is_empty() { "(no text)" } else { &text }
        );
        if page["finish_reason"] == "length" {
            part.push_str("(This page reached DeepSeek's output limit; its text is incomplete.)\n");
        }
        if out.len() + part.len() > budget {
            out += "\n(Pages ";
            out += &number.to_string();
            out += " to ";
            out += &pages.len().to_string();
            out += " are not shown: the text is too long.)";
            break;
        }
        out.push_str(&part);
    }
    out.trim_end().to_string()
}

fn mib(bytes: u64) -> f64 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "文件大小远小于 2^52，只为报错时说个大概"
    )]
    let bytes = bytes as f64;
    bytes / 1024.0 / 1024.0
}

/// 给工具 `data` 的：DeepSeek 的回答原样（带记号）；太大的只留页数这些，免得一行超过核心的 1 MiB。
pub fn data(result: &Value, budget: usize) -> Value {
    if result.to_string().len() <= budget {
        return result.clone();
    }
    json!({
        "id": result["id"],
        "purpose": result["purpose"],
        "page_count": result["page_count"],
        "usage": result["usage"],
        "pages_omitted": true,
    })
}

#[cfg(test)]
mod tests;
