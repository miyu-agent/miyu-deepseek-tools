use super::{encode, parse};

/// 16 位 PCM 的 `fmt ` 块。
pub fn pcm_fmt(channels: u16, rate: u32) -> Vec<u8> {
    let block = channels * 2;
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&1u16.to_le_bytes());
    fmt.extend_from_slice(&channels.to_le_bytes());
    fmt.extend_from_slice(&rate.to_le_bytes());
    fmt.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
    fmt.extend_from_slice(&block.to_le_bytes());
    fmt.extend_from_slice(&16u16.to_le_bytes());
    fmt
}

/// 头里的数据长度换成 `declared`（边合成边发的写法）。
fn with_declared(mut wav: Vec<u8>, declared: u32) -> Vec<u8> {
    let at = wav.windows(4).position(|w| w == b"data").unwrap();
    wav[at + 4..at + 8].copy_from_slice(&declared.to_le_bytes());
    wav
}

#[test]
fn a_plain_wav_reads_back_with_its_duration() {
    let data = vec![1u8; 24_000 * 2 * 3 / 2]; // 24 kHz 单声道 1.5 秒
    let wav = encode(&pcm_fmt(1, 24_000), &data);
    assert_eq!(wav.len(), 44 + data.len());
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(
        u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]]),
        36 + u32::try_from(data.len()).unwrap()
    );
    let read = parse(&wav).unwrap();
    assert_eq!(
        (read.channels, read.rate, read.block_align, read.bits),
        (1, 24_000, 2, 16)
    );
    assert_eq!(read.duration_ms(), 1500);
    assert!(!read.cut_off);
    assert_eq!(read.encode(), wav);
}

#[test]
fn an_unknown_streaming_length_counts_what_arrived_and_gets_rewritten() {
    let data = vec![3u8; 16_000 * 2];
    for declared in [0, 0xFFFF_FFFF, 0x7FFF_FFFF] {
        let streamed = with_declared(encode(&pcm_fmt(1, 16_000), &data), declared);
        let read = parse(&streamed).unwrap();
        assert_eq!(read.duration_ms(), 1000);
        assert!(!read.cut_off);
        assert_eq!(read.encode(), encode(&pcm_fmt(1, 16_000), &data));
    }
}

#[test]
fn a_real_length_that_did_not_all_arrive_is_cut_off() {
    let wav = encode(&pcm_fmt(1, 16_000), &[0u8; 1000]);
    let cut = &wav[..wav.len() - 100];
    let read = parse(cut).unwrap();
    assert!(read.cut_off);
    assert_eq!(read.data.len(), 900);
}

#[test]
fn half_frames_are_dropped_and_other_chunks_skipped() {
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF\0\0\0\0WAVE");
    // 一个奇数长的 LIST 块，后面带填充字节。
    wav.extend_from_slice(b"LIST\x03\0\0\0abc\0");
    let fmt = pcm_fmt(2, 8000);
    wav.extend_from_slice(b"fmt \x10\0\0\0");
    wav.extend_from_slice(&fmt);
    wav.extend_from_slice(b"data\x07\0\0\0");
    wav.extend_from_slice(&[9u8; 7]);
    let read = parse(&wav).unwrap();
    assert_eq!(read.channels, 2);
    assert_eq!(read.data.len(), 4, "两帧，每帧 4 个字节，多的 3 个不要");
}

#[test]
fn not_a_wav_is_an_error() {
    assert!(parse(b"OggS....").is_err());
    assert!(
        parse(b"RIFF\0\0\0\0WAVEdata\x02\0\0\0ab").is_err(),
        "没有 fmt"
    );
    assert!(parse(b"RIFF\0\0\0\0WAVE").is_err());
    let mut zero_rate = pcm_fmt(1, 16_000);
    zero_rate[4..8].copy_from_slice(&0u32.to_le_bytes());
    assert!(parse(&encode(&zero_rate, b"ab")).is_err());
}

#[test]
fn splitting_cuts_on_frames_and_every_piece_is_a_valid_wav() {
    let data: Vec<u8> = (0..=255u8).cycle().take(16_000 * 2 * 5).collect(); // 5 秒
    let wav = encode(&pcm_fmt(1, 16_000), &data);
    let read = parse(&wav).unwrap();
    let pieces = read.split(16_000 * 2); // 每段至多 2 秒
    assert_eq!(pieces.len(), 3);
    let mut joined = Vec::new();
    let durations: Vec<u64> = pieces
        .iter()
        .map(|piece| {
            let part = parse(piece).unwrap();
            assert!(!part.cut_off);
            joined.extend_from_slice(part.data);
            part.duration_ms()
        })
        .collect();
    assert_eq!(durations, [2000, 2000, 1000]);
    assert_eq!(joined, data, "切开再接上一字不差");
}
