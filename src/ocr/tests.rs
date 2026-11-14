use serde_json::json;

use super::{Purpose, data, parse, shown};

#[test]
fn keep_alive_newlines_before_the_json_are_skipped() {
    let answer = b"\n\n\n  \n{\"id\": \"ocr-1\", \"object\": \"ocr.result\", \"page_count\": 1, \"pages\": [{\"index\": 0, \"text\": \"hi\", \"finish_reason\": \"stop\"}]}";
    let result = parse(answer).unwrap();
    assert_eq!(result["pages"][0]["text"], "hi");
}

#[test]
fn an_error_in_a_200_body_is_a_failure() {
    let answer = b"\n\n{\"error\": {\"message\": \"OCR did not finish in time\", \"type\": \"gateway_timeout_error\", \"param\": null, \"code\": \"gateway_timeout_error\"}}";
    assert_eq!(
        parse(answer).unwrap_err(),
        "DeepSeek OCR failed: OCR did not finish in time"
    );
    assert!(parse(b"\n\n").unwrap_err().contains("not JSON"));
    assert!(parse(b"{\"id\": \"x\"}").unwrap_err().contains("no pages"));
}

#[test]
fn pages_are_numbered_from_one_and_cleaned() {
    let result = json!({
        "page_count": 2,
        "pages": [
            {"index": 0, "text": "<｜ref｜>Title<｜/ref｜><｜box｜>[[52, 40, 948, 96]]<｜/box｜>\nBody", "finish_reason": "stop"},
            {"index": 1, "text": "Half a pa", "finish_reason": "length"}
        ]
    });
    assert_eq!(
        shown(&result, usize::MAX),
        "2 pages recognized; DeepSeek bills OCR per page.\n\n\
--- page 1 ---\nTitle\nBody\n\n\
--- page 2 ---\nHalf a pa\n(This page reached DeepSeek's output limit; its text is incomplete.)"
    );
    let one =
        json!({"page_count": 1, "pages": [{"index": 0, "text": "", "finish_reason": "stop"}]});
    assert_eq!(
        shown(&one, usize::MAX),
        "1 page recognized; DeepSeek bills OCR per page.\n\n--- page 1 ---\n(no text)"
    );
}

#[test]
fn too_much_text_stops_at_a_page_and_says_so() {
    let pages: Vec<_> = (0..5)
        .map(|index| json!({"index": index, "text": "x".repeat(100), "finish_reason": "stop"}))
        .collect();
    let result = json!({"page_count": 5, "pages": pages});
    let text = shown(&result, 300);
    assert!(text.contains("--- page 2 ---"));
    assert!(!text.contains("--- page 3 ---"));
    assert!(text.ends_with("(Pages 3 to 5 are not shown: the text is too long.)"));
}

#[test]
fn data_is_the_raw_answer_unless_too_big() {
    let result = json!({"id": "ocr-1", "purpose": "text_recognition", "page_count": 1,
        "pages": [{"index": 0, "text": "<｜ref｜>a<｜/ref｜>", "finish_reason": "stop"}], "usage": {"total_tokens": 3}});
    assert_eq!(data(&result, 10_000), result);
    let small = data(&result, 10);
    assert_eq!(small["pages_omitted"], true);
    assert_eq!(small["page_count"], 1);
    assert!(small.get("pages").is_none());
}

#[test]
fn purposes() {
    assert_eq!(Purpose::parse(&json!(null)).unwrap(), Purpose::Text);
    assert_eq!(
        Purpose::parse(&json!("rich_layout_recognition"))
            .unwrap()
            .as_str(),
        "rich_layout_recognition"
    );
    assert_eq!(
        Purpose::parse(&json!("text_recognition")).unwrap(),
        Purpose::Text
    );
    assert!(Purpose::parse(&json!("layout")).is_err());
}
