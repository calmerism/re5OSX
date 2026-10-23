//! A small HTTP/1.1 server on 127.0.0.1: a thread per connection,
//! keep-alive. Enough for the launcher's page and its requests.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;

pub struct Request {
    pub method: String,
    /// The path, percent-decoded, without the query.
    pub path: String,
    headers: HashMap<String, String>,
    pub body: Vec<u8>,
    /// Came from this Mac (the listener is on 127.0.0.1 only).
    pub local: bool,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(String::as_str)
    }
}

pub enum Body {
    Bytes(Vec<u8>),
    /// A file, or `len` bytes of it from `start`.
    File { file: std::fs::File, start: u64, len: u64 },
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

impl Response {
    pub fn new(status: u16, content_type: &str, body: Vec<u8>) -> Response {
        Response { status, headers: vec![("Content-Type".into(), content_type.into())], body: Body::Bytes(body) }
    }

    pub fn json(value: &serde_json::Value) -> Response {
        Response::new(200, "application/json", value.to_string().into_bytes())
    }

    pub fn json_status(status: u16, value: &serde_json::Value) -> Response {
        Response { status, ..Response::json(value) }
    }

    pub fn error(status: u16, text: &str) -> Response {
        Response::new(status, "text/plain; charset=utf-8", text.as_bytes().to_vec())
    }

    pub fn with(mut self, name: &str, value: impl Into<String>) -> Response {
        self.headers.push((name.into(), value.into()));
        self
    }
}

pub type Handler = Arc<dyn Fn(&Request) -> Response + Send + Sync>;

/// Serves `listener` on threads of its own.
pub fn serve(listener: TcpListener, handler: Handler) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let handler = handler.clone();
            thread::spawn(move || {
                let _ = stream.set_nodelay(true);
                let local = stream.peer_addr().is_ok_and(|p| p.ip().is_loopback());
                connection(stream, local, &handler);
            });
        }
    });
}

fn connection<S: Read + Write>(stream: S, local: bool, handler: &Handler) {
    let mut reader = BufReader::with_capacity(64 * 1024, stream);
    loop {
        let request = match read_request(&mut reader, local) {
            Ok(Some(r)) => r,
            Ok(None) => return,
            Err(_) => {
                let _ = write_response(reader.get_mut(), Response::error(400, "bad request"), false, true);
                return;
            }
        };
        let close = request.header("connection").is_some_and(|c| c.eq_ignore_ascii_case("close"));
        let head = request.method == "HEAD";
        let response = handler(&request);
        if write_response(reader.get_mut(), response, head, close).is_err() || close {
            return;
        }
    }
}

fn read_request<S: Read>(reader: &mut BufReader<S>, local: bool) -> io::Result<Option<Request>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Err(io::ErrorKind::InvalidData.into());
    };
    let (method, target) = (method.to_string(), target.to_string());
    let mut headers = HashMap::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
        if headers.len() > 100 {
            return Err(io::ErrorKind::InvalidData.into());
        }
    }
    let length: usize = headers.get("content-length").and_then(|l| l.parse().ok()).unwrap_or(0);
    if length > 64 * 1024 * 1024 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let path = target.split_once('?').map_or(target.as_str(), |(p, _)| p);
    Ok(Some(Request { method, path: decode(path), headers, body, local }))
}

fn write_response<S: Write>(stream: &mut S, response: Response, head: bool, close: bool) -> io::Result<()> {
    let len = match &response.body {
        Body::Bytes(b) => b.len() as u64,
        Body::File { len, .. } => *len,
    };
    let mut out = format!("HTTP/1.1 {} {}\r\nContent-Length: {len}\r\n", response.status, reason(response.status));
    for (name, value) in &response.headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    if close {
        out.push_str("Connection: close\r\n");
    }
    out.push_str("\r\n");
    stream.write_all(out.as_bytes())?;
    if !head {
        match response.body {
            Body::Bytes(b) => stream.write_all(&b)?,
            Body::File { mut file, start, len } => {
                use std::io::Seek;
                file.seek(io::SeekFrom::Start(start))?;
                io::copy(&mut file.take(len), stream)?;
            }
        }
    }
    stream.flush()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    }
}

/// Percent-decoding.
pub fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn mime(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "wasm" => "application/wasm",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}
