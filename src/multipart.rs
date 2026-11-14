//! `multipart/form-data` 的请求体（RFC 7578），自己拼：ureq 3 的 multipart 放在 `unversioned` 底下、不照 semver，
//! 升一个小版本都可能变。只要两样：几格文字、一个文件。文件夹在 [`Form::head`] 和 [`Form::tail`] 中间，大文件可以边读边发。

use std::hash::{BuildHasher, Hasher};

/// 一份表单除了文件内容以外的部分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    /// 请求头 `Content-Type` 的值，带着分界线。
    pub content_type: String,
    /// 文字的几格，和文件那一格的头。
    pub head: Vec<u8>,
    /// 文件后面的收尾。
    pub tail: Vec<u8>,
}

impl Form {
    /// 几格文字（照先后）加一个文件：`file` 是（格名、文件名、文件的类型）。分界线是 `boundary`。
    pub fn new(boundary: &str, fields: &[(&str, &str)], file: (&str, &str, &str)) -> Form {
        let (field, name, media_type) = file;
        let mut parts: Vec<String> = fields
            .iter()
            .map(|(key, value)| {
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n{value}\r\n",
                    quoted(key)
                )
            })
            .collect();
        parts.push(format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {media_type}\r\n\r\n",
            quoted(field),
            quoted(name)
        ));
        Form {
            content_type: format!("multipart/form-data; boundary={boundary}"),
            head: parts.concat().into_bytes(),
            tail: format!("\r\n--{boundary}--\r\n").into_bytes(),
        }
    }

    /// 文件有 `size` 字节时整个请求体多长（`Content-Length`）。
    pub fn length(&self, size: u64) -> u64 {
        self.head.len() as u64 + size + self.tail.len() as u64
    }

    /// 文件内容在手里的：拼成整个请求体。
    pub fn with(&self, file: &[u8]) -> Vec<u8> {
        let mut body = Vec::with_capacity(self.head.len() + file.len() + self.tail.len());
        body.extend_from_slice(&self.head);
        body.extend_from_slice(file);
        body.extend_from_slice(&self.tail);
        body
    }
}

/// 一条新的分界线：32 位十六进制的随机数（标准库每次给的随机种子），撞上文件内容的机会可以不计。
pub fn boundary() -> String {
    let random = |n: u64| {
        let mut hasher = std::hash::RandomState::new().build_hasher();
        hasher.write_u64(n);
        hasher.finish()
    };
    format!("miyu-deepseek-tools-{:016x}{:016x}", random(0), random(1))
}

/// 放进引号里的名字：引号、反斜杠、换行这些会弄坏头的换成 `_`；文件名只用来认扩展名，非 ASCII 的也换掉，
/// 换完一个字母、数字都不剩的叫 `file`。DeepSeek 照内容认格式（文档「The format is detected from the actual file content」）。
pub fn quoted(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if (c.is_ascii_graphic() && c != '"' && c != '\\') || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if safe.chars().any(|c| c.is_ascii_alphanumeric()) {
        safe
    } else {
        "file".to_string()
    }
}

#[cfg(test)]
mod tests;
