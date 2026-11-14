use serde_json::{Value, json};

use super::harness::{
    KEY, TempDir, call, hooks_pointing_at, registered, reply_text, start, start_with_key, wait_for,
};
use super::{METHODS, NO_KEY, TOOL};
use crate::fake_http::{Answer, FakeDeepSeek, Received};
use crate::multipart::Form;
use crate::wav::{encode, parse, tests::pcm_fmt};

/// 带着 `Authorization: Bearer <测试 key>`。
fn assert_authorized(request: &Received) {
    let expected = format!("Bearer {KEY}");
    assert_eq!(request.header("authorization"), Some(expected.as_str()));
}

/// multipart 的请求体照它自己的分界线重拼一遍，要一字不差；`Content-Length` 写实。
fn assert_form(
    request: &Received,
    fields: &[(&str, &str)],
    file: (&str, &str, &str),
    content: &[u8],
) {
    let boundary = request
        .header("content-type")
        .and_then(|value| value.strip_prefix("multipart/form-data; boundary="))
        .expect("a multipart content type");
    assert_eq!(
        request.body,
        Form::new(boundary, fields, file).with(content)
    );
    let length = request.body.len().to_string();
    assert_eq!(request.header("content-length"), Some(length.as_str()));
}

/// 边合成边发的 WAV：头里的长度写成 `0xFFFFFFFF`。
fn streamed_wav(data: &[u8]) -> Vec<u8> {
    let mut wav = encode(&pcm_fmt(1, 24_000), data);
    wav[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    wav[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
    wav
}

fn ocr_answer(text: &str) -> Value {
    json!({"id": "ocr-1", "object": "ocr.result", "created": 1_762_790_000, "purpose": "rich_layout_recognition",
           "page_count": 1, "pages": [{"index": 0, "text": text, "finish_reason": "stop"}],
           "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}})
}

#[test]
fn synthesize_sends_deepseek_json_and_returns_a_wav_blob() {
    let data = vec![7u8; 24_000]; // 24 kHz 单声道 16 位 0.5 秒
    let server = FakeDeepSeek::start(vec![Answer::new(200, "audio/wav", streamed_wav(&data))]);
    let (core, blobs) = start_with_key(&server.base);
    let got = call(
        &core,
        "tts.engine.synthesize",
        &json!({"text": "好的，三点见。", "voice": "", "speed": 1.2, "format": "mp3"}),
    );
    assert_eq!(got["media_type"], "audio/wav", "{got}");
    assert_eq!(got["duration_ms"], 500);
    assert_eq!(
        blobs.get(got["audio"].as_str().unwrap()),
        encode(&pcm_fmt(1, 24_000), &data),
        "头里的长度写实了"
    );
    let received = server.received();
    assert_eq!(received.len(), 1);
    let request = &received[0];
    assert_eq!(
        (request.method.as_str(), request.path.as_str()),
        ("POST", "/v1/tools/tts")
    );
    assert_authorized(request);
    assert_eq!(request.header("content-type"), Some("application/json"));
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(
        body,
        json!({"text": "好的，三点见。", "voice_id": "stella", "format": "wav", "filter_markdown": false})
    );
    core.close().unwrap();
}

#[test]
fn voices_are_registered_with_the_key() {
    let server = FakeDeepSeek::start(Vec::new());
    let (core, _) = start_with_key(&server.base);
    let got = call(&core, "tts.engine.voices", &json!({}));
    assert_eq!(
        got,
        json!({"voices": [{"id": "stella", "name": "Stella (female)"}, {"id": "tide", "name": "Tide (male)"}]})
    );
    assert!(server.received().is_empty());
    core.close().unwrap();
}

#[test]
fn transcribe_uploads_the_wav_and_returns_the_language_it_was_given() {
    let wav = encode(&pcm_fmt(1, 16_000), &vec![1u8; 16_000 * 2]);
    let server = FakeDeepSeek::start(vec![Answer::new(
        200,
        "application/json",
        r#"{"request_id": "r1", "text": "今天下午三点开会", "duration_ms": 1000}"#,
    )]);
    let (core, blobs) = start_with_key(&server.base);
    let audio = blobs.add(wav.clone());
    let got = call(
        &core,
        "stt.engine.transcribe",
        &json!({"audio": audio, "language": "yue"}),
    );
    assert_eq!(got, json!({"text": "今天下午三点开会", "language": "yue"}));
    let received = server.received();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].path, "/v1/tools/asr");
    assert_authorized(&received[0]);
    assert_form(&received[0], &[], ("file", "audio.wav", "audio/wav"), &wav);
    core.close().unwrap();
}

#[test]
fn long_audio_goes_in_110_second_pieces_and_the_texts_are_joined() {
    let data: Vec<u8> = (0..=200u8).cycle().take(16_000 * 2 * 115).collect();
    let server = FakeDeepSeek::start(vec![
        Answer::new(200, "application/json", r#"{"text": "First part."}"#),
        Answer::new(200, "application/json", r#"{"text": "second part."}"#),
    ]);
    let (core, blobs) = start_with_key(&server.base);
    let audio = blobs.add(encode(&pcm_fmt(1, 16_000), &data));
    let got = call(
        &core,
        "stt.engine.transcribe",
        &json!({"audio": audio, "language": "en"}),
    );
    assert_eq!(
        got,
        json!({"text": "First part. second part.", "language": "en"})
    );
    let received = server.received();
    assert_eq!(received.len(), 2);
    let mut joined = Vec::new();
    let mut durations = Vec::new();
    for request in &received {
        let content_type = request.header("content-type").unwrap();
        let boundary = content_type.split_once("boundary=").unwrap().1;
        let form = Form::new(boundary, &[], ("file", "audio.wav", "audio/wav"));
        let file = &request.body[form.head.len()..request.body.len() - form.tail.len()];
        let piece = parse(file).unwrap();
        durations.push(piece.duration_ms());
        joined.extend_from_slice(piece.data);
    }
    assert_eq!(durations, [110_000, 5_000]);
    assert_eq!(joined, data, "两段接起来就是原来的声音");
    core.close().unwrap();
}

#[test]
fn the_ocr_tool_uploads_the_file_and_shows_cleaned_pages() {
    let dir = TempDir::new("tool");
    let content = b"\x89PNG\r\n\x1a\n not really an image".to_vec();
    std::fs::write(dir.0.join("scan.png"), &content).unwrap();
    let answer =
        ocr_answer("<｜ref｜>Invoice<｜/ref｜><｜box｜>[[1, 2, 3, 4]]<｜/box｜>\nTotal: 42");
    let server = FakeDeepSeek::start(vec![Answer::new(
        200,
        "application/json",
        format!("\n\n\n{answer}"),
    )]);
    let (core, _) = start_with_key(&server.base);
    core.set_cwd(&dir.0);
    let reply = core.call_tool(
        TOOL,
        json!({"path": "scan.png", "purpose": "rich_layout_recognition"}),
    );
    assert!(reply.get("error").is_none(), "{reply}");
    assert_eq!(
        reply_text(&reply),
        "1 page recognized; DeepSeek bills OCR per page.\n\n--- page 1 ---\nInvoice\nTotal: 42"
    );
    assert_eq!(reply["data"], answer, "原样的回答（带记号）给脚本");
    let received = server.received();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].path, "/v1/tools/ocr");
    assert_authorized(&received[0]);
    assert_form(
        &received[0],
        &[("purpose", "rich_layout_recognition")],
        ("file", "scan.png", "application/octet-stream"),
        &content,
    );
    let spec = core
        .tools()
        .into_iter()
        .find(|tool| tool["name"] == TOOL)
        .unwrap();
    assert_eq!(spec["access"], "outbound");
    assert!(
        spec["description"]
            .as_str()
            .unwrap()
            .contains("uploaded to DeepSeek")
    );
    core.close().unwrap();
}

#[test]
fn an_ocr_error_inside_a_200_answer_fails_the_tool() {
    let dir = TempDir::new("error200");
    std::fs::write(dir.0.join("a.pdf"), b"%PDF-1.7").unwrap();
    let server = FakeDeepSeek::start(vec![Answer::new(
        200,
        "application/json",
        "\n\n{\"error\": {\"message\": \"OCR did not finish in time\", \"type\": \"gateway_timeout_error\", \"param\": null, \"code\": \"gateway_timeout_error\"}}",
    )]);
    let (core, _) = start_with_key(&server.base);
    core.set_cwd(&dir.0);
    let reply = core.call_tool(TOOL, json!({"path": "a.pdf"}));
    assert_eq!(reply["error"], true);
    assert_eq!(
        reply_text(&reply),
        "DeepSeek OCR failed: OCR did not finish in time"
    );
    assert_form(
        &server.received()[0],
        &[("purpose", "text_recognition")],
        ("file", "a.pdf", "application/octet-stream"),
        b"%PDF-1.7",
    );
    core.close().unwrap();
}

#[test]
fn a_429_is_engine_failed_and_never_shows_the_key() {
    let busy = format!(r#"{{"error": {{"message": "Too many requests for {KEY}"}}}}"#);
    let server = FakeDeepSeek::start(vec![
        Answer::new(429, "application/json", busy.clone()),
        Answer::new(402, "application/json", busy),
    ]);
    let (core, _) = start_with_key(&server.base);
    let got = call(
        &core,
        "tts.engine.synthesize",
        &json!({"text": "hi", "voice": "tide"}),
    );
    assert_eq!(got["error"]["data"]["code"], "engine_failed", "{got}");
    let message = got["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("429: too many requests in progress"),
        "{message}"
    );
    assert!(message.contains("Too many requests for <key>"), "{message}");
    assert!(!got.to_string().contains(KEY), "{got}");
    // 工具走同一条路：402 余额不足。
    let dir = TempDir::new("402");
    std::fs::write(dir.0.join("a.pdf"), b"%PDF").unwrap();
    core.set_cwd(&dir.0);
    let reply = core.call_tool(TOOL, json!({"path": "a.pdf"}));
    assert_eq!(reply["error"], true);
    assert!(reply_text(&reply).contains("402: the DeepSeek balance is too low"));
    assert!(!reply.to_string().contains(KEY), "{reply}");
    core.close().unwrap();
}

#[test]
fn files_that_cannot_go_are_not_uploaded() {
    let dir = TempDir::new("local");
    let big = std::fs::File::create(dir.0.join("big.pdf")).unwrap();
    big.set_len(crate::ocr::LARGEST + 1).unwrap();
    std::fs::write(dir.0.join("empty.pdf"), b"").unwrap();
    let server = FakeDeepSeek::start(Vec::new());
    let (core, _) = start_with_key(&server.base);
    core.set_cwd(&dir.0);
    for (path, said) in [
        ("big.pdf", "at most 100 MiB"),
        ("empty.pdf", "is empty"),
        ("missing.pdf", "Cannot read"),
    ] {
        let reply = core.call_tool(TOOL, json!({"path": path}));
        assert_eq!(reply["error"], true);
        assert!(reply_text(&reply).contains(said), "{path}: {reply}");
    }
    assert!(server.received().is_empty());
    core.close().unwrap();
}

#[test]
fn without_a_key_the_plugs_stay_off_and_ocr_says_so() {
    let (core, _) = start(Box::new(|_| None));
    let names = registered(&core);
    assert!(names.contains(&"ocr".to_string()) && names.contains(&"info".to_string()));
    assert!(
        METHODS
            .iter()
            .all(|method| !names.contains(&(*method).to_string())),
        "{names:?}"
    );
    let reply = core.call_tool(TOOL, json!({"path": "a.pdf"}));
    assert_eq!(reply["error"], true);
    assert_eq!(reply_text(&reply), NO_KEY);
    // 撤登记之前就到了的调用：回 not_configured。
    let got = call(&core, "tts.engine.synthesize", &json!({"text": "hi"}));
    assert_eq!(got["error"]["data"]["code"], "not_configured");
    assert_eq!(call(&core, "info", &json!({})), json!({"key": false}));
    core.close().unwrap();
}

#[test]
fn a_config_push_turns_the_plugs_on_and_off() {
    let server = FakeDeepSeek::start(Vec::new());
    let hooks = hooks_pointing_at(&server.base);
    let (core, _) = start(Box::new(move |config| {
        config
            .contains_key("test.on")
            .then(|| hooks(config))
            .flatten()
    }));
    assert!(!registered(&core).contains(&"stt.engine.transcribe".to_string()));
    core.notify("extension.config", json!({"keys": {"test.on": true}}));
    for method in METHODS {
        wait_for(&core, method, true);
    }
    assert_eq!(call(&core, "info", &json!({})), json!({"key": true}));
    core.notify("extension.config", json!({"keys": {"test.on": null}}));
    for method in METHODS {
        wait_for(&core, method, false);
    }
    core.close().unwrap();
}

#[test]
fn the_provider_key_pushed_by_the_core_turns_the_plugs_on_and_off() {
    let (core, _) = start(Box::new(|config| {
        crate::credentials::credentials_from(config, &|_| None)
    }));
    assert!(!registered(&core).contains(&"stt.engine.transcribe".to_string()));
    core.notify(
        "extension.config",
        json!({"keys": {"provider_key": "sk-pushed-by-core"}}),
    );
    for method in METHODS {
        wait_for(&core, method, true);
    }
    assert_eq!(call(&core, "info", &json!({})), json!({"key": true}));
    core.notify("extension.config", json!({"keys": {"provider_key": null}}));
    for method in METHODS {
        wait_for(&core, method, false);
    }
    assert_eq!(call(&core, "info", &json!({})), json!({"key": false}));
    core.close().unwrap();
}

#[test]
fn the_command_line_s_ocr_method_hands_back_the_text_as_a_blob() {
    let dir = TempDir::new("method");
    let file = dir.0.join("page.jpg");
    std::fs::write(&file, b"\xff\xd8\xff jpeg").unwrap();
    let server = FakeDeepSeek::start(vec![Answer::new(
        200,
        "application/json",
        ocr_answer("你好").to_string(),
    )]);
    let (core, blobs) = start_with_key(&server.base);
    let relative = call(&core, "ocr", &json!({"path": "page.jpg"}));
    assert_eq!(relative["error"]["data"]["code"], "bad_params");
    let got = call(
        &core,
        "ocr",
        &json!({"path": file, "purpose": "text_recognition"}),
    );
    assert_eq!(got["page_count"], 1, "{got}");
    let text = String::from_utf8(blobs.get(got["text"].as_str().unwrap())).unwrap();
    assert_eq!(
        text,
        "1 page recognized; DeepSeek bills OCR per page.\n\n--- page 1 ---\n你好"
    );
    assert_eq!(server.received().len(), 1);
    core.close().unwrap();
}
