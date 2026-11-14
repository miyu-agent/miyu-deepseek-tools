use super::{Form, boundary, quoted};

#[test]
fn the_body_is_exactly_rfc_7578() {
    let form = Form::new(
        "XyZ",
        &[("purpose", "text_recognition")],
        ("file", "scan.pdf", "application/octet-stream"),
    );
    assert_eq!(form.content_type, "multipart/form-data; boundary=XyZ");
    let file = b"%PDF-1.7 \x00\xff";
    let body = form.with(file);
    let expected = "--XyZ\r\n\
Content-Disposition: form-data; name=\"purpose\"\r\n\
\r\n\
text_recognition\r\n\
--XyZ\r\n\
Content-Disposition: form-data; name=\"file\"; filename=\"scan.pdf\"\r\n\
Content-Type: application/octet-stream\r\n\
\r\n\
@@FILE@@\r\n\
--XyZ--\r\n";
    let (before, after) = expected.split_once("@@FILE@@").unwrap();
    let mut want = before.as_bytes().to_vec();
    want.extend_from_slice(file);
    want.extend_from_slice(after.as_bytes());
    assert_eq!(body, want);
    assert_eq!(form.length(file.len() as u64), body.len() as u64);
}

#[test]
fn a_file_only_form_has_just_the_file_part() {
    let form = Form::new("b", &[], ("file", "audio.wav", "audio/wav"));
    assert_eq!(
        String::from_utf8(form.with(b"RIFF")).unwrap(),
        "--b\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\nRIFF\r\n--b--\r\n"
    );
}

#[test]
fn names_cannot_break_the_header() {
    assert_eq!(quoted("a\"b\\c\r\nd.pdf"), "a_b_c__d.pdf");
    assert_eq!(quoted("报告 2026.docx"), "__ 2026.docx");
    assert_eq!(quoted(""), "file");
    assert_eq!(quoted("\n"), "file");
}

#[test]
fn boundaries_differ_and_are_header_safe() {
    let (a, b) = (boundary(), boundary());
    assert_ne!(a, b);
    assert_eq!(a.len(), "miyu-deepseek-tools-".len() + 32);
    assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
}
