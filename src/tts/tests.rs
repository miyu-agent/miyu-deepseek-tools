use serde_json::json;

use super::{Ask, ask, body, speech, voices};
use crate::wav::{encode, tests::pcm_fmt};

#[test]
fn the_request_body_is_deepseek_s_json() {
    let asked =
        ask(&json!({"text": "好的，三点见。", "voice": "tide", "speed": 1.5, "format": "mp3"}))
            .unwrap();
    assert_eq!(
        body(&asked),
        json!({"text": "好的，三点见。", "voice_id": "tide", "format": "wav", "filter_markdown": false})
    );
}

#[test]
fn an_empty_or_missing_voice_is_stella() {
    for params in [json!({"text": "hi", "voice": ""}), json!({"text": "hi"})] {
        assert_eq!(
            ask(&params).unwrap(),
            Ask {
                text: "hi".into(),
                voice: "stella".into()
            }
        );
    }
}

#[test]
fn wrong_params_are_bad_params() {
    for (params, said) in [
        (json!({"text": "hi", "pitch": 2}), "Unknown field pitch."),
        (json!({"voice": "tide"}), "Give text as a string."),
        (json!({"text": "  "}), "The text is empty."),
        (
            json!({"text": "hi", "voice": "alloy"}),
            "Unknown voice alloy: DeepSeek has stella and tide.",
        ),
        (
            json!({"text": "hi", "speed": "fast"}),
            "Give speed as a number.",
        ),
        (
            json!({"text": "hi", "format": "ogg"}),
            "Unknown format \"ogg\": use mp3 or wav.",
        ),
    ] {
        let error = ask(&params).unwrap_err();
        assert_eq!(error.code.as_deref(), Some("bad_params"));
        assert_eq!(error.message, said);
    }
}

#[test]
fn streamed_wav_gets_a_real_header_and_a_duration() {
    let data = vec![5u8; 24_000 * 2 * 2]; // 24 kHz 单声道 2 秒
    let mut streamed = encode(&pcm_fmt(1, 24_000), &data);
    streamed[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    streamed[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
    let made = speech(&streamed).unwrap();
    assert_eq!(made.duration_ms, 2000);
    assert_eq!(made.wav, encode(&pcm_fmt(1, 24_000), &data));
}

#[test]
fn cut_off_empty_or_foreign_audio_fails() {
    let whole = encode(&pcm_fmt(1, 24_000), &[1u8; 4800]);
    assert!(
        speech(&whole[..whole.len() - 10])
            .unwrap_err()
            .contains("cut off")
    );
    assert!(
        speech(&encode(&pcm_fmt(1, 24_000), &[]))
            .unwrap_err()
            .contains("no audio")
    );
    assert!(
        speech(b"ID3\x04mp3")
            .unwrap_err()
            .contains("did not send WAV")
    );
}

#[test]
fn voices_follow_the_language() {
    assert_eq!(
        voices("zh"),
        json!({"voices": [{"id": "stella", "name": "海星（女声）"}, {"id": "tide", "name": "暗潮（男声）"}]})
    );
    assert_eq!(voices("en")["voices"][1]["name"], "Tide (male)");
    assert_eq!(voices("ja")["voices"][0]["name"], "Stella (female)");
}
