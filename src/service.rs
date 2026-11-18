//! 跑着的扩展：总线上的三个插口方法（`tts.engine.synthesize`、`tts.engine.voices`、`stt.engine.transcribe`），只给自己的
//! 命令行用的 `ocr`、`info`，和三件工具 `deepseek_status`、`deepseek_balance`、`deepseek_ocr`。
//!
//! 插口方法有 key 才登记（`bus.list` 照登记报 ready，朗读、语音输入的配置页才列这一家）：握手以后、每次配置推送都重看一遍
//! （[`Service::refresh`]）。工具一直登记：`deepseek_status` 不用 key；另两件没有 key 时回一句「还没有 key」，什么都不发。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use miyu_ext::{Call, Core, Extension, Method, MethodError, Reply, Tool};
use serde_json::{Map, Value, json};

use crate::credentials::Credentials;
use crate::http::{Api, Body, DeepSeek};
use crate::multipart::{Form, boundary};
use crate::ocr::{self, Purpose};
use crate::params::{bad, only, text};
use crate::{asr, balance, blobs, status, tts};

/// 有 key 才登记的三个插口方法。
pub const METHODS: [&str; 3] = [
    "tts.engine.synthesize",
    "tts.engine.voices",
    "stt.engine.transcribe",
];

/// 工具名。
pub const TOOL: &str = "deepseek_ocr";
pub const STATUS_TOOL: &str = "deepseek_status";
pub const BALANCE_TOOL: &str = "deepseek_balance";

const DESCRIPTION: &str = "Recognize the text in a PDF, Word (DOC, DOCX), PowerPoint (PPT, PPTX) or image file with DeepSeek OCR. The file is uploaded to DeepSeek, which bills per page. Returns the text page by page.";

/// 别的包调方法 `ocr` 时回的。
const OCR_PEOPLE_ONLY: &str =
    "The ocr method is only for people at the command line; other packages cannot call it.";

/// 没有 key 时工具回的。
pub const NO_KEY: &str = "No DeepSeek key yet, so the file was not uploaded.";

/// 给模型看的字至多多少字节、`data` 里原样的回答至多多少：两样加起来留在核心一行 1 MiB 的限以内。
const SHOWN_BUDGET: usize = 400 * 1024;
const DATA_BUDGET: usize = 300 * 1024;

/// 照配置找 key 的办法（[`crate::credentials::credentials`]；测试交自己的）。
pub type Lookup = Box<dyn Fn(&Map<String, Value>) -> Option<Credentials> + Send + Sync>;

/// 跑着的扩展：找 key 的办法、现在的 DeepSeek（没有 key 是空的）。
pub struct Service {
    lookup: Lookup,
    deepseek: RwLock<Option<Arc<DeepSeek>>>,
    /// 一次只有一个 [`Service::refresh`]：握手以后的那次和配置推送的那次撞上时，后读配置的后发登记，不会拿旧的盖掉新的。
    refreshing: Mutex<()>,
}

impl Service {
    pub fn new(lookup: Lookup) -> Arc<Service> {
        Arc::new(Service {
            lookup,
            deepseek: RwLock::new(None),
            refreshing: Mutex::new(()),
        })
    }

    /// 现在的 DeepSeek。
    fn deepseek(&self) -> Option<Arc<DeepSeek>> {
        self.deepseek
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// 方法里用：没有 key 的回 `not_configured`。
    fn configured(&self) -> Result<Arc<DeepSeek>, MethodError> {
        self.deepseek()
            .ok_or_else(|| MethodError::new("not_configured", "No DeepSeek key yet."))
    }

    /// 握手以后、配置变了：重找 key，有了登记三个插口方法，没了撤掉。key 没变的接着用原来的连接池。
    pub fn refresh(&self, core: &Core) {
        let _one_at_a_time = self
            .refreshing
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let found = (self.lookup)(&core.config());
        let ready = found.is_some();
        {
            let mut slot = self
                .deepseek
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            let same = slot.as_ref().map(|deepseek| deepseek.credentials()) == found.as_ref();
            if !same {
                *slot = found.map(|credentials| Arc::new(DeepSeek::new(credentials)));
            }
        }
        let switched = if ready {
            core.enable_methods(&METHODS)
        } else {
            core.disable_methods(&METHODS)
        };
        if let Err(error) = switched {
            eprintln!("miyu-deepseek-tools: package.methods failed: {error}");
        }
    }

    /// 登记方法、工具，握手以后、配置推送时 [`Service::refresh`]。
    pub fn register(self: &Arc<Service>, ext: &mut Extension) {
        let service = Arc::clone(self);
        ext.method_with(
            Method::new("tts.engine.synthesize")
                .timeout_ms(70_000)
                .later(),
            move |params, _, core| service.synthesize(params, core),
        );
        ext.method_with(
            Method::new("tts.engine.voices").timeout_ms(5_000).later(),
            move |params, _, core| {
                only(params, &[])?;
                Ok(tts::voices(&core.language()))
            },
        );
        // 长的录音切成几段一段段传，每段至多等 120 秒：给足核心允许的 10 分钟。
        let service = Arc::clone(self);
        ext.method_with(
            Method::new("stt.engine.transcribe")
                .timeout_ms(600_000)
                .later(),
            move |params, _, core| service.transcribe(params, core),
        );
        // 下面两个只给自己的命令行（`miyu deepseek-tools`）经 package.call 调，不上总线。`ocr` 会把文件传出去，只给人：
        // 别的包调的（核心转来时带着 `from`，调的包的编号）一律拒，清单的 `[export]` 也不列它。
        let service = Arc::clone(self);
        ext.method_with(
            Method::new("ocr").timeout_ms(200_000),
            move |params, from, core| {
                if from.is_some() {
                    return Err(MethodError::new("not_allowed", OCR_PEOPLE_ONLY));
                }
                service.ocr_method(params, core)
            },
        );
        let service = Arc::clone(self);
        ext.method_with(
            Method::new("info").timeout_ms(5_000),
            move |params, _, _| {
                only(params, &[])?;
                Ok(json!({"key": service.deepseek().is_some()}))
            },
        );
        ext.tool(status_tool(), |args, _| {
            let outcome = status::answer(args);
            if outcome.ok {
                Reply::text(outcome.text)
            } else {
                Reply::failed(outcome.text)
            }
        });
        let service = Arc::clone(self);
        ext.tool(balance_tool(), move |args, _| service.balance_tool(args));
        let service = Arc::clone(self);
        ext.tool(tool(), move |args, call| service.ocr_tool(args, call));
        let service = Arc::clone(self);
        ext.ready(move |core| service.refresh(core));
        // 配置推送的处理函数在开发包自己的一个线程上跑，可以等核心回 package.methods。
        let service = Arc::clone(self);
        ext.on_config(move |_, core| service.refresh(core));
    }

    fn synthesize(&self, params: &Value, core: &Core) -> Result<Value, MethodError> {
        let asked = tts::ask(params)?;
        let deepseek = self.configured()?;
        let body = tts::body(&asked).to_string().into_bytes();
        let answer = deepseek
            .post(Api::Tts, "application/json", Body::Bytes(body))
            .map_err(engine_failed)?;
        let speech = tts::speech(&answer).map_err(engine_failed)?;
        let blob = blobs::put(core, &speech.wav, "speech.wav", "audio/wav")
            .map_err(|error| engine_failed(format!("Could not store the audio: {error}.")))?;
        Ok(json!({"audio": blob, "media_type": "audio/wav", "duration_ms": speech.duration_ms}))
    }

    fn transcribe(&self, params: &Value, core: &Core) -> Result<Value, MethodError> {
        only(params, &["audio", "language"])?;
        let blob = text(params, "audio")?;
        let language = text(params, "language")?;
        let deepseek = self.configured()?;
        let audio = blobs::get(core, blob).map_err(|why| MethodError::new("bad_audio", why))?;
        let pieces = asr::pieces(&audio).map_err(|why| MethodError::new("bad_audio", why))?;
        let mut texts = Vec::with_capacity(pieces.len());
        for piece in pieces {
            let form = Form::new(&boundary(), &[], ("file", piece.name, piece.media_type));
            let answer = deepseek
                .post(
                    Api::Asr,
                    &form.content_type,
                    Body::Bytes(form.with(&piece.bytes)),
                )
                .map_err(engine_failed)?;
            texts.push(asr::text(&answer).map_err(|why| engine_failed(deepseek.scrub(&why)))?);
        }
        Ok(json!({"text": asr::join(&texts), "language": language}))
    }

    fn balance_tool(&self, args: &Value) -> Reply {
        if let Some(unknown) = args.as_object().and_then(|fields| fields.keys().next()) {
            return Reply::failed(format!(
                "Unknown parameter {unknown}: {BALANCE_TOOL} takes no parameters."
            ));
        }
        let Some(deepseek) = self.deepseek() else {
            return Reply::failed(balance::NO_KEY);
        };
        match balance::check(&deepseek) {
            Ok((text, answer)) => Reply::text(text).data(answer),
            Err(why) => Reply::failed(why),
        }
    }

    fn ocr_tool(&self, args: &Value, call: &Call) -> Reply {
        let Some(deepseek) = self.deepseek() else {
            return Reply::failed(NO_KEY);
        };
        let purpose = match Purpose::parse(&args["purpose"]) {
            Ok(purpose) => purpose,
            Err(why) => return Reply::failed(why),
        };
        // 核心判过的真实位置；老的核心不给的，照工作区自己拼。
        let path = if let Some(checked) = call.checked().first() {
            PathBuf::from(checked)
        } else {
            Path::new(call.cwd()).join(args["path"].as_str().unwrap_or_default())
        };
        match ocr::recognize(&deepseek, &path, purpose) {
            Ok(result) => {
                Reply::text(ocr::shown(&result, SHOWN_BUDGET)).data(ocr::data(&result, DATA_BUDGET))
            }
            Err(why) => Reply::failed(why),
        }
    }

    /// 命令行的 `miyu deepseek-tools ocr`：认出来的整份字存成 blob 交回（几百页的会超过一行 1 MiB）。
    fn ocr_method(&self, params: &Value, core: &Core) -> Result<Value, MethodError> {
        only(params, &["path", "purpose"])?;
        let path = PathBuf::from(text(params, "path")?);
        if !path.is_absolute() {
            return Err(bad("Give the file as an absolute path."));
        }
        let purpose = Purpose::parse(&params["purpose"]).map_err(|why| bad(&why))?;
        let deepseek = self.configured()?;
        let result = ocr::recognize(&deepseek, &path, purpose)
            .map_err(|why| MethodError::new("ocr_failed", why))?;
        let shown = ocr::shown(&result, usize::MAX);
        let blob =
            blobs::put(core, shown.as_bytes(), "ocr.txt", "text/plain").map_err(|error| {
                MethodError::new("ocr_failed", format!("Could not store the text: {error}."))
            })?;
        Ok(json!({"page_count": result["page_count"], "text": blob}))
    }
}

/// 工具 `deepseek_ocr` 的规格：会把文件传出这台电脑，access 写 `outbound`。
fn tool() -> Tool {
    Tool::new(TOOL, DESCRIPTION)
        .schema(json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "The file, relative to the workspace or absolute. At most 100 MiB."},
                "purpose": {
                    "type": "string",
                    "enum": ["text_recognition", "rich_layout_recognition"],
                    "description": "text_recognition (default) reads the text of Word and PowerPoint files without images directly and recognizes other files page by page; rich_layout_recognition recognizes every file page by page."
                }
            },
            "required": ["path"]
        }))
        .access("outbound")
        .paths(json!([{"parameter": "path"}]))
        .display_name(json!({"en": "DeepSeek OCR", "zh": "DeepSeek 文字识别"}))
        .subject("path")
        .timeout_ms(200_000)
}

/// 工具 `deepseek_status`（照原来的脚本：联网只读，脚本里能 `miyu call`，30 秒）：不用 key。
fn status_tool() -> Tool {
    Tool::new(STATUS_TOOL, status::DESCRIPTION)
        .schema(status::schema())
        .access("network")
        .display_name(json!({"en": "DeepSeek status", "zh": "DeepSeek 状态查询"}))
        .timeout_ms(30_000)
        .clicall(true)
}

/// 工具 `deepseek_balance`：联网只读，没有参数；原样的回答在 `data` 里，脚本里能 `miyu call`。
fn balance_tool() -> Tool {
    Tool::new(BALANCE_TOOL, balance::DESCRIPTION)
        .schema(json!({"type": "object", "properties": {}, "additionalProperties": false}))
        .access("network")
        .display_name(json!({"en": "DeepSeek balance", "zh": "DeepSeek 余额查询"}))
        .timeout_ms(40_000)
        .clicall(true)
}

/// 合成、识别出了错：`engine_failed`（bus.md：提供方包成 `engine_failed`，`message` 照引擎的）。
fn engine_failed(message: impl Into<String>) -> MethodError {
    MethodError::new("engine_failed", message)
}

#[cfg(test)]
mod harness;
#[cfg(test)]
mod more_tests;
#[cfg(test)]
mod tests;
