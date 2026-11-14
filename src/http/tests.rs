use super::{Api, failure, loopback, said};

#[test]
fn status_codes_become_short_english_with_deepseek_s_own_words() {
    let busy = failure(
        Api::Tts,
        429,
        br#"{"error": {"message": "Too many requests", "type": "rate_limit"}}"#,
    );
    assert_eq!(
        busy,
        "DeepSeek text to speech failed (429: too many requests in progress on this DeepSeek account): Too many requests"
    );
    assert_eq!(
        failure(Api::Asr, 413, b""),
        "DeepSeek speech to text failed (413: the audio file is larger than 8 MiB)."
    );
    assert!(
        failure(Api::Ocr, 402, b"Insufficient Balance")
            .ends_with("(402: the DeepSeek balance is too low): Insufficient Balance")
    );
    assert!(failure(Api::Ocr, 504, b"").contains("the upload and submission took too long"));
    assert!(failure(Api::Asr, 408, b"").contains("10 minutes"));
    assert!(failure(Api::Tts, 504, b"").contains("synthesis timed out"));
    assert!(failure(Api::Tts, 400, b"").contains("content review"));
    assert!(failure(Api::Ocr, 500, b"").contains("could not take the file"));
    assert!(failure(Api::Asr, 503, b"").contains("busy"));
    assert!(failure(Api::Tts, 401, b"").contains("refused the key"));
    assert!(failure(Api::Tts, 502, b"").contains("server error"));
    assert!(failure(Api::Tts, 418, b"").contains("unexpected answer"));
}

#[test]
fn deepseek_s_words_come_from_json_or_plain_text_in_one_line() {
    assert_eq!(
        said(b"\n\n {\"error\": {\"message\": \"OCR did not finish in time\"}}").as_deref(),
        Some("OCR did not finish in time")
    );
    assert_eq!(said(br#"{"message": "bad"}"#).as_deref(), Some("bad"));
    assert_eq!(said(br#"{"error": "plain"}"#).as_deref(), Some("plain"));
    assert_eq!(
        said(b"<html>\n  Bad   Gateway\n</html>").as_deref(),
        Some("<html> Bad Gateway </html>")
    );
    assert_eq!(said(b"  \n"), None);
    assert_eq!(said(br#"{"other": 1}"#), None);
    assert_eq!(
        said("长".repeat(1000).as_bytes()).unwrap().chars().count(),
        300
    );
}

#[test]
fn only_this_machine_skips_the_proxy() {
    assert!(loopback("http://127.0.0.1:4000"));
    assert!(loopback("http://localhost/x"));
    assert!(loopback("http://[::1]:80"));
    assert!(!loopback("https://api.deepseek.com"));
    assert!(!loopback("https://127.example.com"));
}
