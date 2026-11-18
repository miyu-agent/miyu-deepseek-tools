use super::{PIECE_BYTES, join, pieces, text};
use crate::wav::{encode, parse, tests::pcm_fmt};

/// 16 kHz 单声道 16 位、`seconds` 秒的 WAV。
fn speech(seconds: usize) -> Vec<u8> {
    let data: Vec<u8> = (0..=250u8).cycle().take(16_000 * 2 * seconds).collect();
    encode(&pcm_fmt(1, 16_000), &data)
}

#[test]
fn short_audio_is_one_piece() {
    let audio = speech(3);
    let got = pieces(&audio).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].bytes, audio);
    assert_eq!((got[0].name, got[0].media_type), ("audio.wav", "audio/wav"));
}

#[test]
fn audio_longer_than_110_seconds_is_split_on_frames() {
    let audio = speech(250);
    let got = pieces(&audio).unwrap();
    let durations: Vec<u64> = got
        .iter()
        .map(|piece| parse(&piece.bytes).unwrap().duration_ms())
        .collect();
    assert_eq!(durations, [110_000, 110_000, 30_000]);
    let joined: Vec<u8> = got
        .iter()
        .flat_map(|piece| parse(&piece.bytes).unwrap().data.to_vec())
        .collect();
    assert_eq!(joined, parse(&audio).unwrap().data, "接起来一字不差");
}

#[test]
fn big_frames_split_by_size_under_8_mib() {
    // 48 kHz 立体声 16 位：60 秒就有 11 MB，要照 8 MiB 切，不照 110 秒。
    let data = vec![0u8; 48_000 * 4 * 60];
    let audio = encode(&pcm_fmt(2, 48_000), &data);
    let got = pieces(&audio).unwrap();
    assert!(got.len() > 1);
    assert!(got.iter().all(|piece| piece.bytes.len() <= PIECE_BYTES));
}

#[test]
fn ogg_goes_whole_and_other_audio_is_refused() {
    let ogg = pieces(b"OggS\0\x02rest").unwrap();
    assert_eq!((ogg[0].name, ogg[0].media_type), ("audio.ogg", "audio/ogg"));
    assert!(pieces(b"ID3\x03").is_err());
    assert!(
        pieces(&encode(&pcm_fmt(1, 16_000), &[]))
            .unwrap_err()
            .contains("empty")
    );
    let mut eight_bit = pcm_fmt(1, 16_000);
    eight_bit[12..14].copy_from_slice(&1u16.to_le_bytes());
    eight_bit[14..16].copy_from_slice(&8u16.to_le_bytes());
    assert!(
        pieces(&encode(&eight_bit, b"abc"))
            .unwrap_err()
            .contains("8-bit")
    );
}

#[test]
fn the_answer_s_text_or_its_error() {
    assert_eq!(
        text(
            br#"{"request_id": "r1", "text": " What is the weather like? ", "duration_ms": 3200}"#
        )
        .unwrap(),
        "What is the weather like?"
    );
    assert_eq!(
        text(br#"{"error": {"message": "busy"}}"#).unwrap_err(),
        "DeepSeek speech to text failed: busy"
    );
    assert!(text(b"nope").is_err());
    assert!(text(b"{}").unwrap_err().contains("no text"));
}

#[test]
fn pieces_join_with_spaces_only_between_spaced_scripts() {
    let texts = |list: &[&str]| list.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    assert_eq!(
        join(&texts(&["今天下午", "三点开会。"])),
        "今天下午三点开会。"
    );
    assert_eq!(join(&texts(&["See you", "at three."])), "See you at three.");
    assert_eq!(join(&texts(&["开会", "", "OK"])), "开会 OK");
    assert_eq!(join(&texts(&["Привет", "мир"])), "Привет мир");
    assert_eq!(join(&texts(&[])), "");
}

/// `fmt ` 块：16 位 PCM 的那 16 个字节后面补零，补到 `len` 个字节。
fn padded_fmt(len: usize) -> Vec<u8> {
    let mut fmt = pcm_fmt(1, 16_000);
    fmt.resize(len, 0);
    fmt
}

#[test]
fn a_format_block_too_big_for_a_piece_is_refused() {
    for len in [PIECE_BYTES, PIECE_BYTES - 44, PIECE_BYTES - 45] {
        let audio = encode(&padded_fmt(len), &[1, 2, 3, 4]);
        let got = pieces(&audio);
        assert!(
            got.as_ref().is_err_and(|why| why.contains("format block")),
            "fmt {len}: {got:?}"
        );
    }
    // 还放得下整帧的：照常切，每段不超过 8 MiB。
    let audio = encode(&padded_fmt(PIECE_BYTES - 46), &[1, 2, 3, 4]);
    let got = pieces(&audio).unwrap();
    assert_eq!(got.len(), 2);
    assert!(got.iter().all(|piece| piece.bytes.len() <= PIECE_BYTES));
}
