//! WAV 文件：读头（几声道、采样率、一帧几个字节、数据在哪），算时长，按帧切成几段、每段重写头。合成交回的、识别要传的
//! 都是 WAV，不另加依赖。
//!
//! 边合成边发的 WAV，头里的数据长度还不知道，常写成 `0` 或 `0xFFFFFFFF`：这种照实际收到的算。写了真长度、收到的又不够的
//! 是传到一半断了（DeepSeek 文档：合成中途出错时回应被截断，要当失败）。

/// 头里写「长度还不知道」的那几种写法：`0`、`0x7FFFFFFF` 以上。
const UNKNOWN_FROM: u32 = 0x7FFF_FFFF;

/// 读出来的一个 WAV，借着原来的字节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wav<'a> {
    /// `fmt ` 块的原样内容，重写头时照抄。
    pub fmt: &'a [u8],
    pub channels: u16,
    pub rate: u32,
    /// 一帧（各声道各一个样本）几个字节。
    pub block_align: u16,
    pub bits: u16,
    /// 声音数据：只取整帧。
    pub data: &'a [u8],
    /// 头里写了数据有多长、收到的不够。
    pub cut_off: bool,
}

impl Wav<'_> {
    /// 有几帧。
    pub fn frames(&self) -> u64 {
        (self.data.len() / usize::from(self.block_align)) as u64
    }

    /// 多长，毫秒（不足一毫秒的舍掉）。
    pub fn duration_ms(&self) -> u64 {
        self.frames() * 1000 / u64::from(self.rate)
    }

    /// 重新拼成一个头、长度都写实的 WAV。
    pub fn encode(&self) -> Vec<u8> {
        encode(self.fmt, self.data)
    }

    /// 按帧切成一段段，每段至多 `frames` 帧，各自带着写实的头。没有数据的交回空的。
    pub fn split(&self, frames: usize) -> Vec<Vec<u8>> {
        let size = frames.max(1) * usize::from(self.block_align);
        self.data
            .chunks(size)
            .map(|piece| encode(self.fmt, piece))
            .collect()
    }
}

/// 读一个 WAV 的头。
///
/// # Errors
///
/// 不是 RIFF/WAVE、没有 `fmt `、`data` 块，`fmt ` 太短，不是 PCM，声道、采样率、帧长是 0：一句英文。
pub fn parse(bytes: &[u8]) -> Result<Wav<'_>, String> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("It is not a WAV file.".to_string());
    }
    let mut fmt: Option<&[u8]> = None;
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let declared =
            u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]);
        let start = at + 8;
        let available = bytes.len() - start;
        if id == b"data" {
            let fmt = fmt.ok_or("The WAV file has its data before its format.")?;
            let unknown = declared == 0 || declared >= UNKNOWN_FROM;
            let wanted = usize::try_from(declared).unwrap_or(usize::MAX);
            let taken = if unknown {
                available
            } else {
                wanted.min(available)
            };
            return described(
                fmt,
                &bytes[start..start + taken],
                !unknown && wanted > available,
            );
        }
        let size = usize::try_from(declared).unwrap_or(usize::MAX);
        if size > available {
            break;
        }
        if id == b"fmt " {
            fmt = Some(&bytes[start..start + size]);
        }
        // 块照偶数字节对齐：长度是奇数的后面多一个填充字节。
        at = start + size + (size & 1);
    }
    Err("The WAV file has no audio data.".to_string())
}

/// 照 `fmt ` 块的内容读声道、采样率、帧长、位深，数据去掉不成整帧的尾巴。
fn described<'a>(fmt: &'a [u8], data: &'a [u8], cut_off: bool) -> Result<Wav<'a>, String> {
    if fmt.len() < 16 {
        return Err("The WAV format block is too short.".to_string());
    }
    let u16_at = |at: usize| u16::from_le_bytes([fmt[at], fmt[at + 1]]);
    if !is_pcm(fmt) {
        return Err(format!(
            "The WAV audio is not PCM (format tag {:#06x}).",
            u16_at(0)
        ));
    }
    let channels = u16_at(2);
    let rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
    let block_align = u16_at(12);
    let bits = u16_at(14);
    if channels == 0 || rate == 0 || block_align == 0 {
        return Err("The WAV format block says zero channels, rate or frame size.".to_string());
    }
    let whole = data.len() - data.len() % usize::from(block_align);
    Ok(Wav {
        fmt,
        channels,
        rate,
        block_align,
        bits,
        data: &data[..whole],
        cut_off,
    })
}

/// 是不是 PCM：格式标签 1，或者 `0xFFFE`（`WAVE_FORMAT_EXTENSIBLE`）且子格式是 PCM（`fmt ` 块第 24 到 40 个字节，
/// `00000001-0000-0010-8000-00aa00389b71`）。DeepSeek 识别收 16 位 PCM 的 WAV，别的编码切出来也是乱的。
fn is_pcm(fmt: &[u8]) -> bool {
    const PCM_SUBFORMAT: [u8; 16] = [
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b,
        0x71,
    ];
    match u16::from_le_bytes([fmt[0], fmt[1]]) {
        1 => true,
        0xFFFE => fmt.get(24..40) == Some(&PCM_SUBFORMAT[..]),
        _ => false,
    }
}

/// 拼一个 WAV：`RIFF` 头、`fmt ` 块（`fmt` 照抄）、`data` 块，长度都写实，奇数长的块后面补一个字节。
pub fn encode(fmt: &[u8], data: &[u8]) -> Vec<u8> {
    let padded = |len: usize| len + (len & 1);
    let total = 4 + 8 + padded(fmt.len()) + 8 + padded(data.len());
    let mut out = Vec::with_capacity(total + 8);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&u32_len(total).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    for (id, body) in [(b"fmt ", fmt), (b"data", data)] {
        out.extend_from_slice(id);
        out.extend_from_slice(&u32_len(body.len()).to_le_bytes());
        out.extend_from_slice(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
    }
    out
}

/// 写进头里的长度：WAV 只有 32 位，超了的写满（这里切出来的段都远小于 4 GiB）。
fn u32_len(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

#[cfg(test)]
pub mod tests;
