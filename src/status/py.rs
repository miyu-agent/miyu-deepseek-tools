//! 照 Python 的规矩办的几样，`deepseek_status` 的输出才和原来的脚本（`deepseek_status.py`）一字不差：真假、`str()`、字典的键、
//! 浮点数的 `repr`、`str.strip()`、银行家舍入、`json.dumps` 的分隔符、北京时间（`datetime.fromtimestamp` 的舍入）。

use std::io;

use serde_json::Value;
use serde_json::ser::Formatter;

/// Python 的真假：`None`、`False`、`0`、`0.0`、空字、空列表、空字典是假。
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(yes) => *yes,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}

/// Python 的 `a or b`。
pub fn or<'a>(first: &'a Value, second: &'a Value) -> &'a Value {
    if truthy(first) { first } else { second }
}

/// 字典的 `.get(key)`：不是字典、没有这一格的是 `None`。
pub fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}

/// Python 的 `str()`（f-string 里放的就是它）。
pub fn str_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => repr(other),
    }
}

/// Python 的 `repr()`：字照 Python 的引号规矩，列表、字典照 Python 的写法。
pub fn repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(number) => match (number.as_i64(), number.as_u64()) {
            (Some(int), _) => int.to_string(),
            (None, Some(int)) => int.to_string(),
            _ => float_repr(number.as_f64().unwrap_or_default()),
        },
        Value::String(text) => string_repr(text),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(fields) => {
            let inner: Vec<String> = fields
                .iter()
                .map(|(key, value)| format!("{}: {}", string_repr(key), repr(value)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// 字的 `repr`：有单引号没双引号的用双引号，不然用单引号；反斜杠、换行这几样转义。
fn string_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let n = c as usize;
                out.push_str("\\x");
                out.push(char::from(HEX[n >> 4]));
                out.push(char::from(HEX[n & 0xf]));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// 浮点数的 `repr`：最短能读回原数的那几位；`1e-4` 到 `1e16` 之间写小数（整数也带 `.0`），别的写科学计数法
/// （`1e-05`、`1.5e+16`：指数带符号、至少两位）。
pub fn float_repr(number: f64) -> String {
    if number.is_nan() {
        return "nan".to_string();
    }
    if number.is_infinite() {
        return if number > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let shortest = format!("{number:e}");
    let (mantissa, exponent) = shortest.split_once('e').unwrap_or((&shortest, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    if !(-4..16).contains(&exponent) {
        let mantissa = if digits.len() > 1 {
            format!("{}.{}", &digits[..1], &digits[1..])
        } else {
            digits
        };
        let exponent_sign = if exponent < 0 { '-' } else { '+' };
        return format!("{sign}{mantissa}e{exponent_sign}{:02}", exponent.abs());
    }
    if exponent < 0 {
        let zeros = "0".repeat(usize::try_from(-exponent - 1).unwrap_or(0));
        return format!("{sign}0.{zeros}{digits}");
    }
    let point = usize::try_from(exponent + 1).unwrap_or(0);
    if digits.len() <= point {
        format!("{sign}{digits}{}.0", "0".repeat(point - digits.len()))
    } else {
        format!("{sign}{}.{}", &digits[..point], &digits[point..])
    }
}

/// 字典的键：`None`、数（`True` 当 `1`，`1` 和 `1.0` 是同一个键）、字。列表、字典不能当键（Python 在这里会出错），交回空的。
pub fn key(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("None".to_string()),
        Value::Bool(yes) => Some(format!("i:{}", u8::from(*yes))),
        Value::Number(number) => match (number.as_i64(), number.as_u64()) {
            (Some(int), _) => Some(format!("i:{int}")),
            (None, Some(int)) => Some(format!("i:{int}")),
            _ => {
                let float = number.as_f64().unwrap_or_default();
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "只在这个浮点数正好是整数、放得进 i64 时换，和 Python 里 1 == 1.0 一样"
                )]
                let as_int = float as i64;
                if float.fract() == 0.0 && float.abs() < 9.2e18 {
                    Some(format!("i:{as_int}"))
                } else {
                    Some(format!("n:{float:?}"))
                }
            }
        },
        Value::String(text) => Some(format!("s:{text}")),
        Value::Array(_) | Value::Object(_) => None,
    }
}

/// 照数比大小用的值（排序、取最大）：数照数，`True`、`False` 是 1、0，别的当 0。
pub fn number(value: &Value) -> f64 {
    match value {
        Value::Bool(yes) => f64::from(u8::from(*yes)),
        Value::Number(number) => number.as_f64().unwrap_or_default(),
        _ => 0.0,
    }
}

/// `str.strip()`：Python 认的空白比 Rust 多 `\x1c` 到 `\x1f` 四个。
pub fn strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// Python 的 `round()`：一半的时候取偶数。
pub fn round(number: f64) -> i64 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "这里舍入的是几千以内的分钟数"
    )]
    let rounded = number.round_ties_even() as i64;
    rounded
}

/// `json.dumps(…, ensure_ascii=False)` 的样子：逗号、冒号后面带空格，浮点数照 [`float_repr`]。转义的规矩 `serde_json` 和
/// Python 一样（`"`、`\`、控制字符；别的字照原样）。
pub struct PythonJson;

impl Formatter for PythonJson {
    fn begin_array_value<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_key<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b": ")
    }

    fn write_f64<W: ?Sized + io::Write>(&mut self, writer: &mut W, value: f64) -> io::Result<()> {
        writer.write_all(float_repr(value).as_bytes())
    }
}

/// 照 [`PythonJson`] 写成一行。
pub fn dumps(value: &Value) -> String {
    let mut out = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, PythonJson);
    // 往 Vec 里写不会失败；万一失败交回空的，调的一方照空的处理。
    if serde::Serialize::serialize(value, &mut serializer).is_err() {
        return String::new();
    }
    String::from_utf8(out).unwrap_or_default()
}

/// 北京时间的一刻：年月日时分，和不足一秒的微秒（只用来算时长）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment {
    /// 1970 年以来的秒（UTC）。
    pub seconds: i64,
    pub micros: i64,
}

impl Moment {
    /// 现在。
    pub fn now() -> Moment {
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Moment {
            seconds: i64::try_from(since.as_secs()).unwrap_or(0),
            micros: i64::from(since.subsec_micros()),
        }
    }

    /// `datetime.fromtimestamp(秒, 北京时间)`：小数部分照 `CPython` 舍到微秒（一半取偶数），舍满一秒进位。
    pub fn from_timestamp(value: f64) -> Moment {
        let whole = value.trunc();
        let mut micros = ((value - whole) * 1e6).round_ties_even();
        let mut whole = whole;
        if micros >= 1e6 {
            micros -= 1e6;
            whole += 1.0;
        } else if micros < 0.0 {
            micros += 1e6;
            whole -= 1.0;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "时间戳和微秒都远在 i64 以内"
        )]
        let moment = Moment {
            seconds: whole as i64,
            micros: micros as i64,
        };
        moment
    }

    /// 北京时间的年、月、日、时、分。
    pub fn civil(self) -> (i64, u32, u32, u32, u32) {
        let local = self.seconds + 8 * 3600;
        let days = local.div_euclid(86_400);
        let of_day = local.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        let hour = u32::try_from(of_day / 3600).unwrap_or(0);
        let minute = u32::try_from(of_day % 3600 / 60).unwrap_or(0);
        (year, month, day, hour, minute)
    }

    /// `%Y-%m-%d %H:%M`。
    pub fn date_minute(self) -> String {
        let (year, month, day, hour, minute) = self.civil();
        format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
    }

    /// `%m-%d %H:%M`。
    pub fn month_minute(self) -> String {
        let (_, month, day, hour, minute) = self.civil();
        format!("{month:02}-{day:02} {hour:02}:{minute:02}")
    }

    /// `isoformat(timespec="minutes")`：`2026-11-14T13:05+08:00`。
    pub fn iso_minute(self) -> String {
        let (year, month, day, hour, minute) = self.civil();
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}+08:00")
    }

    /// 到 `later` 有几秒：`int((later - self).total_seconds())`，往零取整。
    pub fn seconds_until(self, later: Moment) -> i64 {
        let micros = (later.seconds - self.seconds) * 1_000_000 + (later.micros - self.micros);
        micros / 1_000_000
    }
}

/// 1970-01-01 以来第几天是哪年哪月哪日（公历，Howard Hinnant 的算法）。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let of_era = z.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let day_of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = u32::try_from(day_of_year - (153 * shifted + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    })
    .unwrap_or(1);
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests;
