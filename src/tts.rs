//! 合成（总线 `tts.engine`，核心 `docs/reference/bus.md`）：一句纯文字交给 DeepSeek 的 `POST /v1/tools/tts`，拿回 WAV。
//!
//! - DeepSeek 只出 `wav`、`pcm`，`format: "mp3"` 照样交 WAV、`media_type` 写实（bus.md：出不了那种的交出得了的）。
//! - 不支持调语速：`speed` 收下、不用。
//! - 交来的已经是念得出来的纯文字，`filter_markdown` 关掉，免得 DeepSeek 再删字。
//! - 边合成边发：头里的长度可能没写实，交出去前重写（[`crate::wav`]）；写了长度又没收全的是中途断了，当失败。

use miyu_ext::MethodError;
use serde_json::{Value, json};

use crate::params::{bad, only, text};
use crate::wav;

/// DeepSeek 的音色：编号、英文名、中文名（DeepSeek 文档的叫法）。
pub const VOICES: [(&str, &str, &str); 2] = [
    ("stella", "Stella (female)", "海星（女声）"),
    ("tide", "Tide (male)", "暗潮（男声）"),
];
/// 没给音色时用的：DeepSeek 自己的默认。
pub const DEFAULT_VOICE: &str = "stella";

/// 查过的一次合成请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub text: String,
    pub voice: String,
}

/// 查 `tts.engine.synthesize` 的参数：`text` 必写、不能是空的；`voice` 是空的照默认；`speed` 是数；`format` 是 `mp3`、`wav`。
///
/// # Errors
///
/// `bad_params`：不认识的字段、类型不对、没有字、不认识的音色。
pub fn ask(params: &Value) -> Result<Ask, MethodError> {
    only(params, &["text", "voice", "speed", "format"])?;
    let words = text(params, "text")?;
    if words.trim().is_empty() {
        return Err(bad("The text is empty."));
    }
    let voice = match &params["voice"] {
        Value::Null => "",
        Value::String(voice) => voice.trim(),
        _ => return Err(bad("Give voice as a string.")),
    };
    let voice = if voice.is_empty() {
        DEFAULT_VOICE
    } else {
        voice
    };
    if !VOICES.iter().any(|(id, _, _)| *id == voice) {
        return Err(bad(&format!(
            "Unknown voice {voice}: DeepSeek has stella and tide."
        )));
    }
    if !matches!(params["speed"], Value::Null | Value::Number(_)) {
        return Err(bad("Give speed as a number."));
    }
    match &params["format"] {
        Value::Null => {}
        Value::String(format) if format == "mp3" || format == "wav" => {}
        other => return Err(bad(&format!("Unknown format {other}: use mp3 or wav."))),
    }
    Ok(Ask {
        text: words.to_string(),
        voice: voice.to_string(),
    })
}

/// 发给 DeepSeek 的 JSON。
pub fn body(ask: &Ask) -> Value {
    json!({"text": ask.text, "voice_id": ask.voice, "format": "wav", "filter_markdown": false})
}

/// 合成好的一段：头写实的 WAV、多长。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speech {
    pub wav: Vec<u8>,
    pub duration_ms: u64,
}

/// 读 DeepSeek 交回的字节。
///
/// # Errors
///
/// 不是 WAV、中途断了、没有声音：一句英文。
pub fn speech(bytes: &[u8]) -> Result<Speech, String> {
    let wav = wav::parse(bytes)
        .map_err(|why| format!("DeepSeek text to speech did not send WAV audio: {why}"))?;
    if wav.cut_off {
        return Err("DeepSeek text to speech stopped partway: the audio is cut off.".to_string());
    }
    if wav.data.is_empty() {
        return Err("DeepSeek text to speech sent no audio.".to_string());
    }
    Ok(Speech {
        duration_ms: wav.duration_ms(),
        wav: wav.encode(),
    })
}

/// `tts.engine.voices` 的回答：名字照人的语言（中文的写中文名，别的写英文名）。
pub fn voices(language: &str) -> Value {
    let voices: Vec<Value> = VOICES
        .iter()
        .map(|(id, en, zh)| json!({"id": id, "name": if language == "zh" { zh } else { en }}))
        .collect();
    json!({"voices": voices})
}

#[cfg(test)]
mod tests;
