//! 连 DeepSeek 那一下：POST 一段请求体（查余额是 GET），交回回应的字节。走系统代理（`HTTPS_PROXY` 这些，`NO_PROXY` 照认）；接口地址是本机的
//! （测试钩子指到假的服务上）不走代理。对方回 4xx/5xx 的照接口、状态码说一句英文，带上 DeepSeek 自己的报错原因。
//!
//! key 只在请求头里：报错从不带请求头，万一对方的报错里带了 key 也换成 `<key>`（[`DeepSeek::scrub`]）。

use std::io::Read;
use std::time::Duration;

use serde_json::Value;
use ureq::{Agent, SendBody};

use crate::credentials::Credentials;

/// 回应最多读多少：一句话的音频、一份几百页的识别结果都远小于它；超了的截断，当对方出了错。
const LONGEST: u64 = 64 * 1024 * 1024;
/// 报错时带对方原话的多少字。
const EXCERPT: usize = 300;

/// 用到的三个接口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    Tts,
    Asr,
    Ocr,
    Balance,
}

impl Api {
    /// 接在接口地址后面的路径。
    pub fn path(self) -> &'static str {
        match self {
            Api::Tts => "/v1/tools/tts",
            Api::Asr => "/v1/tools/asr",
            Api::Ocr => "/v1/tools/ocr",
            // 查余额不在 /v1 底下（DeepSeek 文档 GET /user/balance）。
            Api::Balance => "/user/balance",
        }
    }

    /// 一次请求最多等多久（连上、传完、等回应、读完）：OCR 的 180 秒是 DeepSeek 自己的总时限。
    pub fn timeout(self) -> Duration {
        Duration::from_secs(match self {
            Api::Tts => 60,
            Api::Asr => 120,
            Api::Ocr => 180,
            Api::Balance => 30,
        })
    }

    /// 报错里说是哪一样。
    fn label(self) -> &'static str {
        match self {
            Api::Tts => "text to speech",
            Api::Asr => "speech to text",
            Api::Ocr => "OCR",
            Api::Balance => "balance check",
        }
    }

    /// DeepSeek 文档里这个接口各个状态码的原因。
    fn cause(self, status: u16) -> Option<&'static str> {
        Some(match (self, status) {
            (Api::Tts, 400) => "no readable text, or the text did not pass content review",
            (Api::Asr, 400) => "the audio is not 16-bit PCM WAV or Ogg Opus",
            (Api::Ocr, 400) => "the form or purpose is invalid, or the file cannot be processed",
            // 余额用完对每个接口都是 402（DeepSeek 错误码文档）：原来只写了 OCR，合成、识别报「an unexpected answer」（DS-10）。
            (_, 402) => "the DeepSeek balance is too low",
            (Api::Asr, 408) => "the upload took longer than 10 minutes",
            (Api::Ocr, 408) => "the upload was too slow",
            (Api::Asr, 413) => "the audio file is larger than 8 MiB",
            (Api::Ocr, 413) => "the file is larger than 100 MiB",
            (_, 429) => "too many requests in progress on this DeepSeek account",
            (Api::Ocr, 500) => "DeepSeek could not take the file",
            (_, 503) => "DeepSeek is busy, try again later",
            (Api::Tts, 504) => "synthesis timed out",
            (Api::Asr, 504) => "recognition timed out",
            (Api::Ocr, 504) => "the upload and submission took too long",
            (_, 401 | 403) => "DeepSeek refused the key",
            (_, 404) => "no such DeepSeek API at this address",
            _ => return None,
        })
    }
}

/// 请求体：一整段字节，或者边读边发的（OCR 的文件可能上百 MiB，不整个读进内存），后者要先知道多长。
pub enum Body {
    Bytes(Vec<u8>),
    Stream {
        reader: Box<dyn Read + Send>,
        length: u64,
    },
}

/// 带着 key 的 DeepSeek：几个请求共用一个连接池。
pub struct DeepSeek {
    credentials: Credentials,
    agent: Agent,
}

impl DeepSeek {
    pub fn new(credentials: Credentials) -> DeepSeek {
        DeepSeek {
            agent: agent_for(&credentials.base),
            credentials,
        }
    }

    /// 用的哪一把 key、哪个地址。
    pub fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    /// POST 到 `api`，`content_type` 是请求体的类型。
    ///
    /// # Errors
    ///
    /// 连不上、超时、对方回的不是 2xx（[`failure`]）、读到一半断了：一句英文，不带 key。
    pub fn post(&self, api: Api, content_type: &str, body: Body) -> Result<Vec<u8>, String> {
        let url = format!("{}{}", self.credentials.base, api.path());
        let request = self
            .agent
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.credentials.key))
            .header("Content-Type", content_type)
            .config()
            .timeout_global(Some(api.timeout()))
            .build();
        let sent = match body {
            Body::Bytes(bytes) => request.send(bytes.as_slice()),
            Body::Stream { reader, length } => request
                .header("Content-Length", length.to_string())
                .send(SendBody::from_owned_reader(reader)),
        };
        self.answer(api, sent)
    }

    /// GET `api`（查余额）。
    ///
    /// # Errors
    ///
    /// 同 [`DeepSeek::post`]。
    pub fn get(&self, api: Api) -> Result<Vec<u8>, String> {
        let url = format!("{}{}", self.credentials.base, api.path());
        let sent = self
            .agent
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.credentials.key))
            .config()
            .timeout_global(Some(api.timeout()))
            .build()
            .call();
        self.answer(api, sent)
    }

    /// 读回应：不是 2xx 的照 [`failure`] 说。
    fn answer(
        &self,
        api: Api,
        sent: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<Vec<u8>, String> {
        let response = sent.map_err(|error| {
            self.scrub(&format!(
                "Could not reach DeepSeek {}: {error}.",
                api.label()
            ))
        })?;
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(LONGEST)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                self.scrub(&format!(
                    "The answer from DeepSeek {} broke off: {error}.",
                    api.label()
                ))
            })?;
        if !(200..300).contains(&status) {
            return Err(self.scrub(&failure(api, status, &bytes)));
        }
        Ok(bytes)
    }

    /// 万一字里带了 key（对方的报错原样转过来的），换掉。
    pub fn scrub(&self, text: &str) -> String {
        text.replace(&self.credentials.key, "<key>")
    }
}

/// 对方回的不是 2xx：哪个接口、状态码、文档里的原因、DeepSeek 自己说的。
pub fn failure(api: Api, status: u16, body: &[u8]) -> String {
    let cause = api.cause(status).unwrap_or(if status >= 500 {
        "a DeepSeek server error"
    } else {
        "an unexpected answer"
    });
    match said(body) {
        Some(said) => format!(
            "DeepSeek {} failed ({status}: {cause}): {said}",
            api.label()
        ),
        None => format!("DeepSeek {} failed ({status}: {cause}).", api.label()),
    }
}

/// DeepSeek 自己的报错原因：JSON 的 `error.message`（OCR 文档的写法）、`message`、`detail`，不是 JSON 的取原话的头一段，
/// 并成一行。什么都没有的是空的。
pub fn said(body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    let text = text.trim();
    let message = match serde_json::from_str::<Value>(text) {
        Ok(json) => [
            &json["error"]["message"],
            &json["error"],
            &json["message"],
            &json["detail"],
        ]
        .into_iter()
        .find_map(|value| value.as_str().map(str::to_string))?,
        Err(_) => text.to_string(),
    };
    let line: String = message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(EXCERPT)
        .collect();
    (!line.is_empty()).then_some(line)
}

/// 连 `base` 用的：对方回 4xx/5xx 不当传输错误（照状态码说），走系统代理；本机的地址不走代理。
pub fn agent_for(base: &str) -> Agent {
    let mut config = Agent::config_builder().http_status_as_error(false);
    if loopback(base) {
        config = config.proxy(None);
    }
    config.build().into()
}

/// 接口地址是不是本机（`localhost`、`127.x`、`[::1]`）。
fn loopback(base: &str) -> bool {
    let rest = base.split_once("://").map_or(base, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or(rest);
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests;
