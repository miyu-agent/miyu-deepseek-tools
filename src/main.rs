//! miyu-deepseek-tools：Miyu 的 DeepSeek 功能全家桶（包编号 `deepseek-tools`）。用 DeepSeek 的接口（<https://api-docs.deepseek.com>）
//! 和官方状态页：
//!
//! - 工具 `deepseek_status`：查 DeepSeek 的官方状态页，不用 key（[`status`](mod@status)，照 miyu-scripts 的 Python 脚本搬过来）。
//! - 工具 `deepseek_balance`：查账户余额（[`balance`]）。
//! - 语音合成接进总线的 `tts.engine`、语音识别接进 `stt.engine`（核心 `docs/reference/bus.md`），朗读（`say`）、语音输入
//!   （`voice-input`）在配置里挑它就用它；有 DeepSeek 的 key 才登记（[`service`]）。
//! - 工具 `deepseek_ocr`：把 PDF、Word、PowerPoint、图片传给 DeepSeek 认字，逐页交回（[`ocr`]），DeepSeek 照页收钱。
//!
//! key 现在只从测试钩子读，核心交供应商 key 的能力（PK-1）合进来以后改成读核心交的（[`credentials`]）。
//!
//! - `miyu-deepseek-tools serve`：核心拉起的扩展。
//! - `miyu deepseek-tools`：印在跑的扩展有没有 key、怎么用；`miyu deepseek-tools ocr <文件> [--layout]`：经在跑的扩展认一个文件。命令
//!   自己不拿 key、不连 DeepSeek（[`client`]）。

mod asr;
mod balance;
mod blobs;
mod client;
mod credentials;
#[cfg(test)]
mod fake_http;
mod http;
mod multipart;
mod ocr;
mod params;
mod service;
mod status;
mod tts;
mod wav;

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use miyu_ext::Extension;
use serde_json::{Value, json};

use crate::client::Client;
use crate::service::Service;

const USAGE: &str =
    "usage: miyu deepseek-tools                          whether the DeepSeek key is set
       miyu deepseek-tools ocr <file> [--layout]   recognize the text in a file with DeepSeek OCR
                                                   (uploads the file; DeepSeek bills per page)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let done = match args.first().map(String::as_str) {
        Some("serve") => {
            serve();
            return ExitCode::SUCCESS;
        }
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("ocr") => ocr_command(&args[1..]),
        None => {
            status();
            Ok(())
        }
        Some(other) => Err(format!("unknown command {other}\n{USAGE}")),
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("miyu deepseek-tools: {why}");
            ExitCode::FAILURE
        }
    }
}

fn serve() {
    let service: Arc<Service> = Service::new(Box::new(credentials::credentials));
    let mut ext = Extension::new("miyu-deepseek-tools", env!("CARGO_PKG_VERSION"));
    service.register(&mut ext);
    ext.run();
}

/// 连上核心：数据根照 `MIYU_HOME`（`miyu deepseek-tools` 转过来时设好了）。
fn connect() -> Result<Client, String> {
    let home = std::env::var_os("MIYU_HOME")
        .ok_or("MIYU_HOME is not set; run it as `miyu deepseek-tools`.")?;
    Client::connect(Path::new(&home), "miyu-deepseek-tools")
}

/// `miyu deepseek-tools`：版本、在跑的扩展有没有 key、用法。
fn status() {
    println!("DeepSeek Toolkit {}", env!("CARGO_PKG_VERSION"));
    let line = match connect().and_then(|mut core| {
        core.call(
            "package.call",
            &json!({"package": "deepseek-tools", "method": "info"}),
        )
    }) {
        Ok(info) if info["key"] == true => {
            "key: set (text to speech and speech to text are on the bus)".to_string()
        }
        Ok(_) => "key: not set yet (text to speech, speech to text and OCR are off)".to_string(),
        Err(why) => format!("key: unknown, the extension is not reachable ({why})"),
    };
    println!("{line}\n\n{USAGE}");
}

/// `miyu deepseek-tools ocr <文件> [--layout]`：交给在跑的扩展认，印出认出来的字。
fn ocr_command(args: &[String]) -> Result<(), String> {
    let mut file = None;
    let mut purpose = "text_recognition";
    for arg in args {
        match arg.as_str() {
            "--layout" => purpose = "rich_layout_recognition",
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}\n{USAGE}")),
            path if file.is_none() => file = Some(path),
            _ => return Err(format!("give one file\n{USAGE}")),
        }
    }
    let file = file.ok_or_else(|| format!("give a file\n{USAGE}"))?;
    let path = std::fs::canonicalize(file).map_err(|error| format!("{file}: {error}"))?;
    let path = path
        .to_str()
        .ok_or_else(|| format!("{file}: the path is not valid UTF-8"))?;
    let mut core = connect()?;
    let answer: Value = core.call(
        "package.call",
        &json!({"package": "deepseek-tools", "method": "ocr", "params": {"path": path, "purpose": purpose}}),
    )?;
    let text = core.blob(answer["text"].as_str().unwrap_or_default())?;
    println!("{}", String::from_utf8_lossy(&text));
    Ok(())
}
