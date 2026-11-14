//! 去掉 DeepSeek 逐页认出来的字里的定位记号，给模型、给人看。
//!
//! DeepSeek 文档的例子：`<｜ref｜>Title<｜/ref｜><｜box｜>[[52, 40, 948, 96]]<｜/box｜>`，`ref` 里是那块的字，`box` 里是坐标。
//! 照 DeepSeek-OCR 模型的写法也认 `<｜det｜>…<｜/det｜>`（坐标）和半角的 `<|ref|>`。去法：
//!
//! - `box`、`det` 连同里面的坐标整个去掉；
//! - `ref` 去掉记号、留下里面的字；只有一种例外：它单独占一行、里面是小写的类别名（`title`、`text`、`image` 这种 DeepSeek-OCR
//!   的版面类别），真正的字在下一行，这一行整个去掉；
//! - 别的 `<｜名字｜>` 记号去掉。不像记号的 `<`、`|` 照原样。
//!
//! 最后每行去掉行尾空白，连着的空行并成一行。

/// 一段：普通的字，或者一个记号（名字）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Piece<'a> {
    Text(&'a str),
    Token(&'a str),
}

/// 去掉记号。
pub fn clean(raw: &str) -> String {
    let pieces = split(raw);
    let mut out = String::new();
    let mut eat_newline = false;
    let mut at = 0;
    while at < pieces.len() {
        match pieces[at] {
            Piece::Text(text) => {
                let text = if eat_newline {
                    let rest = text.trim_start_matches([' ', '\t']);
                    rest.strip_prefix("\r\n")
                        .or_else(|| rest.strip_prefix('\n'))
                        .unwrap_or(text)
                } else {
                    text
                };
                eat_newline = false;
                out.push_str(text);
                at += 1;
            }
            Piece::Token("ref") => {
                let mut inside = String::new();
                at += 1;
                while let Some(piece) = pieces.get(at) {
                    at += 1;
                    match piece {
                        Piece::Token("/ref") => break,
                        Piece::Text(text) => inside.push_str(text),
                        Piece::Token(_) => {}
                    }
                }
                while matches!(pieces.get(at), Some(Piece::Token("box" | "det"))) {
                    at = past_region(&pieces, at);
                }
                if starts_line(&out) && ends_line(pieces.get(at)) && is_label(&inside) {
                    out.truncate(out.trim_end_matches([' ', '\t']).len());
                    eat_newline = true;
                } else {
                    out.push_str(&inside);
                }
            }
            Piece::Token("box" | "det") => at = past_region(&pieces, at),
            Piece::Token(_) => at += 1,
        }
    }
    tidy(&out)
}

/// 切成字和记号。
fn split(raw: &str) -> Vec<Piece<'_>> {
    let mut pieces = Vec::new();
    let mut plain = 0;
    let mut at = 0;
    while let Some(found) = raw[at..].find('<') {
        let start = at + found;
        match token(&raw[start..]) {
            Some((name, length)) => {
                if plain < start {
                    pieces.push(Piece::Text(&raw[plain..start]));
                }
                pieces.push(Piece::Token(name));
                at = start + length;
                plain = at;
            }
            None => at = start + 1,
        }
    }
    if plain < raw.len() {
        pieces.push(Piece::Text(&raw[plain..]));
    }
    pieces
}

/// `s` 开头是不是一个记号 `<｜名字｜>`（全角竖线）或 `<|名字|>`：交回名字和整个记号多长。名字只有字母、数字、`_`、`/`、
/// `▁`（DeepSeek 记号里的空格），至多 40 个。
fn token(s: &str) -> Option<(&str, usize)> {
    let after = s.strip_prefix("<｜").or_else(|| s.strip_prefix("<|"))?;
    let opened = s.len() - after.len();
    let bar = after.find(['｜', '|'])?;
    let name = &after[..bar];
    let fits = !name.is_empty()
        && name.chars().count() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '/' | '▁'));
    if !fits {
        return None;
    }
    let rest = &after[bar..];
    let closing = ["｜>", "|>"]
        .into_iter()
        .find(|end| rest.starts_with(end))?;
    Some((name, opened + bar + closing.len()))
}

/// 从 `at`（一个 `box`、`det`）跳到它收尾的记号后面；没收尾的只跳过这一个记号。
fn past_region(pieces: &[Piece<'_>], at: usize) -> usize {
    let Some(Piece::Token(name)) = pieces.get(at) else {
        return at + 1;
    };
    let close = format!("/{name}");
    pieces[at + 1..]
        .iter()
        .position(|piece| *piece == Piece::Token(&close))
        .map_or(at + 1, |offset| at + 1 + offset + 1)
}

/// 写到这里是不是在一行的开头（前面只有空格）。
fn starts_line(out: &str) -> bool {
    let trimmed = out.trim_end_matches([' ', '\t']);
    trimmed.is_empty() || trimmed.ends_with('\n')
}

/// 接下来是不是这一行的末尾。
fn ends_line(next: Option<&Piece<'_>>) -> bool {
    match next {
        None => true,
        Some(Piece::Text(text)) => {
            let rest = text.trim_start_matches([' ', '\t']);
            rest.is_empty() || rest.starts_with('\n') || rest.starts_with("\r\n")
        }
        Some(Piece::Token(_)) => false,
    }
}

/// 像不像版面类别名：小写字母、`_`。
fn is_label(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

/// 每行去掉行尾空白，连着的空行并成一行，头尾的空行去掉。
fn tidy(text: &str) -> String {
    let mut out = String::new();
    let mut blank = false;
    for line in text.lines().map(str::trim_end) {
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if blank {
            out.push('\n');
            blank = false;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests;
