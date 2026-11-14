//! 识别（总线 `stt.engine`，核心 `docs/reference/bus.md`）：一段 WAV 交给 DeepSeek 的 `POST /v1/tools/asr`（multipart 的
//! `file` 一格），拿回字。
//!
//! DeepSeek 一次至多 8 MiB、120 秒。语音输入交来的最长 10 分钟，所以长过 110 秒的按帧切成几段（每段重写头，也不超过
//! 8 MiB），照先后一段段识别，字接起来。DeepSeek 没有语言参数，自己认；回答里的 `language` 照收到的原样交回。

use crate::wav;

/// 一段最长多少秒：比 DeepSeek 的 120 秒留些余量。
pub const PIECE_SECONDS: u32 = 110;
/// 一段最大多少字节：8 MiB 留 64 KiB 余量。
pub const PIECE_BYTES: usize = 8 * 1024 * 1024 - 64 * 1024;

/// 要传的一段：内容、文件名、类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub bytes: Vec<u8>,
    pub name: &'static str,
    pub media_type: &'static str,
}

/// 把交来的音频切成要传的几段。DeepSeek 也收 Ogg Opus：那种不切，整个传（不超过 8 MiB）。
///
/// # Errors
///
/// 不是 WAV（也不是 Ogg）、不是 16 位、没有声音、Ogg 太大：一句英文（[`crate::service`] 报 `bad_audio`）。
pub fn pieces(audio: &[u8]) -> Result<Vec<Piece>, String> {
    if audio.starts_with(b"OggS") {
        if audio.len() > PIECE_BYTES {
            return Err(
                "The Ogg audio is larger than 8 MiB, which DeepSeek takes in one go.".to_string(),
            );
        }
        return Ok(vec![Piece {
            bytes: audio.to_vec(),
            name: "audio.ogg",
            media_type: "audio/ogg",
        }]);
    }
    let wav = wav::parse(audio)?;
    if wav.bits != 16 {
        return Err(format!(
            "The audio is {}-bit; DeepSeek takes 16-bit PCM WAV.",
            wav.bits
        ));
    }
    if wav.data.is_empty() {
        return Err("The audio is empty.".to_string());
    }
    let by_time =
        usize::try_from(u64::from(wav.rate) * u64::from(PIECE_SECONDS)).unwrap_or(usize::MAX);
    let by_size = (PIECE_BYTES - 44 - wav.fmt.len()) / usize::from(wav.block_align);
    Ok(wav
        .split(by_time.min(by_size))
        .into_iter()
        .map(|bytes| Piece {
            bytes,
            name: "audio.wav",
            media_type: "audio/wav",
        })
        .collect())
}

/// DeepSeek 的回答里的字（`{"request_id", "text", "duration_ms"}`）。
///
/// # Errors
///
/// 不是 JSON、带着 `error`、没有 `text`：一句英文。
pub fn text(answer: &[u8]) -> Result<String, String> {
    let json: serde_json::Value = serde_json::from_slice(answer)
        .map_err(|_| "DeepSeek speech to text sent an answer that is not JSON.".to_string())?;
    if !json["error"].is_null() {
        let said = crate::http::said(answer).unwrap_or_else(|| "no reason given".to_string());
        return Err(format!("DeepSeek speech to text failed: {said}"));
    }
    json["text"]
        .as_str()
        .map(|text| text.trim().to_string())
        .ok_or_else(|| "DeepSeek speech to text sent no text.".to_string())
}

/// 几段的字接起来：两边都是中日韩这类不用空格的字的直接接，不然中间放一个空格；空的段跳过。
pub fn join(texts: &[String]) -> String {
    let mut out = String::new();
    for text in texts
        .iter()
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
    {
        let joins_tight = matches!(
            (out.chars().last(), text.chars().next()),
            (Some(left), Some(right)) if unspaced(left) && unspaced(right)
        );
        if !out.is_empty() && !joins_tight {
            out.push(' ');
        }
        out.push_str(text);
    }
    out
}

/// 词和词之间不放空格的文字：中文、日文假名、全角标点、泰文。韩文照英文放空格。
fn unspaced(c: char) -> bool {
    matches!(u32::from(c), 0x0E00..=0x0E7F | 0x3000..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x2_0000..=0x3_FFFF)
}

#[cfg(test)]
mod tests;
