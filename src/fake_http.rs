//! 测试用的假 DeepSeek：在 127.0.0.1 随便一个口上听，一个连接答一个请求（`Connection: close`），照先后回排好的回答，
//! 记下收到的每个请求（方法、路径、请求头、请求体）。不连真的 DeepSeek：没有 key，也花主人的钱。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// 排好的一个回答。
pub struct Answer {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Answer {
    pub fn new(status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) -> Answer {
        Answer {
            status,
            content_type,
            body: body.into(),
        }
    }
}

/// 收到的一个请求。
#[derive(Debug, Clone)]
pub struct Received {
    pub method: String,
    pub path: String,
    /// 请求头，名字小写。
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Received {
    /// 一个请求头的值（名字不分大小写）。
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// 跑着的假服务。
pub struct FakeDeepSeek {
    /// 给 `MIYU_DEEPSEEK_TEST_BASE_URL` 的地址。
    pub base: String,
    received: Arc<Mutex<Vec<Received>>>,
}

impl FakeDeepSeek {
    /// 起来，照先后答 `answers`，答完就不再听。
    pub fn start(answers: Vec<Answer>) -> FakeDeepSeek {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port on 127.0.0.1");
        let base = format!("http://{}", listener.local_addr().expect("the port"));
        let received = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&received);
        std::thread::spawn(move || {
            for answer in answers {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                if let Some(request) = serve(stream, &answer) {
                    log.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(request);
                }
            }
        });
        FakeDeepSeek { base, received }
    }

    /// 到现在收到的请求。
    pub fn received(&self) -> Vec<Received> {
        self.received
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// 读一个请求，答上，关掉连接。
fn serve(stream: TcpStream, answer: &Answer) -> Option<Received> {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .ok()?;
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut words = line.split_whitespace();
    let method = words.next()?.to_string();
    let path = words.next()?.to_string();
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok()?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':')?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    let find = |name: &str| {
        headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    let body = if let Some(length) = find("content-length") {
        let mut body = vec![0; length.parse().ok()?];
        reader.read_exact(&mut body).ok()?;
        body
    } else if find("transfer-encoding").is_some_and(|value| value.contains("chunked")) {
        chunked(&mut reader)?
    } else {
        Vec::new()
    };
    let mut stream = stream;
    let head = format!(
        "HTTP/1.1 {} Fake\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        answer.status,
        answer.content_type,
        answer.body.len()
    );
    stream.write_all(head.as_bytes()).ok()?;
    stream.write_all(&answer.body).ok()?;
    stream.flush().ok()?;
    Some(Received {
        method,
        path,
        headers,
        body,
    })
}

/// 读分块传的请求体。
fn chunked(reader: &mut impl BufRead) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let mut size = String::new();
        reader.read_line(&mut size).ok()?;
        let size = usize::from_str_radix(size.trim().split(';').next()?, 16).ok()?;
        let mut chunk = vec![0; size + 2];
        reader.read_exact(&mut chunk).ok()?;
        if size == 0 {
            return Some(body);
        }
        body.extend_from_slice(&chunk[..size]);
    }
}
