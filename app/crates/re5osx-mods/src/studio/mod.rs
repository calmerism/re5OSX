//! re5OSX Studio's server: the web page (embedded), a WebSocket for the
//! Studio (files, mods, logs, eval) and one for the type checker (LSP).
//! Plain `std::net`, a thread per connection: a handful of clients at most.
//!
//! Whoever reaches the server runs code in the game, so every route but
//! the page itself needs the token (`?token=` or `Authorization: Bearer`),
//! and WebSockets must come from the page's own origin (a page elsewhere in
//! the same browser could otherwise drive the game through localhost).

mod api;
mod assets;
mod lsp;
mod session;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use api::GUIDE;

/// The Studio's default port.
pub const PORT: u16 = 47110;

/// A running server; dropping it stops it.
pub struct Studio {
    port: u16,
    token: String,
    lan: bool,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Studio {
    /// Serves on `port` (0: any), on every interface when `lan`, else only
    /// for this device; `code` is the pairing code (see [`new_code`]).
    pub fn start(port: u16, lan: bool, code: &str) -> std::io::Result<Studio> {
        let ip = if lan { Ipv4Addr::UNSPECIFIED } else { Ipv4Addr::LOCALHOST };
        let listener = TcpListener::bind(SocketAddr::from((ip, port)))?;
        let port = listener.local_addr()?.port();
        let token = normalize(code).unwrap_or_else(new_code);
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Server { token: token.clone(), stop: stop.clone(), clients: AtomicU32::new(0) });
        let stopping = stop.clone();
        let thread = std::thread::Builder::new().name("studio".into()).spawn(move || {
            re5osx_runtime::exit::panic_recoverable();
            for stream in listener.incoming() {
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                let Ok(stream) = stream else {
                    // Out of descriptors, or a socket the system took back
                    // (iOS, in the background): not a busy loop meanwhile.
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                };
                // A thread each, a few at most (a browser opens about six).
                if shared.clients.fetch_add(1, Ordering::AcqRel) >= MAX_CLIENTS {
                    shared.clients.fetch_sub(1, Ordering::AcqRel);
                    continue;
                }
                let server = shared.clone();
                let spawned = std::thread::Builder::new().name("studio client".into()).spawn(move || {
                    // A panic here ends this connection, not the game.
                    re5osx_runtime::exit::panic_recoverable();
                    struct Leaving<'a>(&'a AtomicU32);
                    impl Drop for Leaving<'_> {
                        fn drop(&mut self) {
                            self.0.fetch_sub(1, Ordering::AcqRel);
                        }
                    }
                    let _leaving = Leaving(&server.clients);
                    if let Err(e) = server.serve(stream) {
                        log::debug!("studio: {e}");
                    }
                });
                if spawned.is_err() {
                    shared.clients.fetch_sub(1, Ordering::AcqRel);
                }
            }
        })?;
        log::info!("studio: serving on port {port}{}", if lan { " (network)" } else { " (this device)" });
        Ok(Studio { port, token, lan, stop, thread: Some(thread) })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The pairing code, also the token (`ABCD-EF12` as shown; either form works).
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The code as people type it.
    ///
    /// (The code is the token.)
    pub fn pairing_code(&self) -> String {
        format!("{}-{}", &self.token[..4], &self.token[4..])
    }

    /// The page's address on `host` (an address of this device), token included.
    pub fn url(&self, host: &str) -> String {
        format!("http://{host}:{}/?token={}", self.port, self.token)
    }

    /// Whether other devices may connect.
    pub fn lan(&self) -> bool {
        self.lan
    }
}

impl Drop for Studio {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wake the accept loop.
        let _ = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, self.port)), Duration::from_secs(1));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// 8 characters of Crockford's base32: 40 bits.
/// A code as typed (`abcd-1234`) as the token it is (`ABCD1234`), when it
/// is one: 8 characters of the codes' alphabet.
pub fn normalize(code: &str) -> Option<String> {
    let t: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase();
    (t.len() == 8 && t.bytes().all(|b| ALPHABET.contains(&b))).then_some(t)
}

const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A new pairing code: 8 characters of Crockford's base32 (40 bits; each
/// wrong one costs a second, a few connections at a time).
pub fn new_code() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("random");
    bytes.iter().map(|b| ALPHABET[(b & 31) as usize] as char).collect()
}

/// How many sessions use the type checker: it goes when none does.
static CHECKER_USERS: AtomicU32 = AtomicU32::new(0);

fn checker_users(delta: i32) {
    if delta > 0 {
        CHECKER_USERS.fetch_add(1, Ordering::AcqRel);
    } else if delta < 0 {
        let left = CHECKER_USERS.try_update(Ordering::AcqRel, Ordering::Acquire, |n| Some(n.saturating_sub(1)));
        if left.unwrap_or(0) <= 1 {
            crate::checker::Checker::release();
        }
    }
}

struct Server {
    token: String,
    /// The server stopped (the Studio was turned off, or its code changed):
    /// connections still open end too.
    stop: Arc<AtomicBool>,
    /// Connections being served.
    clients: AtomicU32,
}

/// Connections served at once.
const MAX_CLIENTS: u32 = 24;

/// A request's head and body.
pub(crate) struct Request {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The body's length (read only for requests that take one, once the
    /// code is checked).
    pub length: usize,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

const MAX_HEAD: usize = 32 << 10;
const MAX_BODY: usize = 8 << 20;

impl Server {
    fn serve(&self, stream: TcpStream) -> std::io::Result<()> {
        stream.set_nodelay(true)?;
        // A client that stops reading lets go of its connection too.
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        let mut reader = BufReader::new(Timed { stream: stream.try_clone()?, until: None });
        let mut out = stream;
        loop {
            if self.stop.load(Ordering::Acquire) {
                return Ok(());
            }
            let Some(mut request) = read_request(&mut reader)? else { return Ok(()) };
            // A body only for those that take one, from whoever has the code.
            if request.length > 0 {
                let takes = request.method == "POST" && request.path == "/import";
                if !takes || !self.authorized(&request) {
                    return respond(&mut out, if takes { 401 } else { 413 }, "text/plain", b"pair with the game first", &[]);
                }
                (&mut reader).take(request.length as u64).read_to_end(&mut request.body)?;
                if request.body.len() != request.length {
                    return Ok(());
                }
            }
            let close = request.header("connection").is_some_and(|c| c.eq_ignore_ascii_case("close"));
            let upgrade = request.header("upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
            match (request.method.as_str(), request.path.as_str()) {
                ("GET", "/studio" | "/lsp") if upgrade => {
                    if !self.authorized(&request) || !same_origin(&request) {
                        return respond(&mut out, 401, "text/plain", b"pair with the game first", &[]);
                    }
                    // The handshake is done: the reader's buffer is empty
                    // (a client waits for the answer before it sends).
                    drop(reader);
                    let ws = accept(out, &request)?;
                    if request.path == "/lsp" {
                        // Released when the last one goes, however it goes.
                        struct User;
                        impl Drop for User {
                            fn drop(&mut self) {
                                checker_users(-1);
                            }
                        }
                        checker_users(1);
                        let _user = User;
                        return lsp::run(ws, &self.stop);
                    }
                    return session::run(ws, &self.stop);
                }
                // A mod as a zip, to download; one from a zip, uploaded.
                ("GET", "/export") => match request.param("mod").map(|m| m.to_string()) {
                    Some(name) if self.authorized(&request) => match crate::files::export(&name) {
                        Ok(zip) => {
                            let disposition = format!("attachment; filename=\"{name}.zip\"");
                            respond(&mut out, 200, "application/zip", &zip, &[("Content-Disposition", &disposition)])?;
                        }
                        Err(e) => respond(&mut out, 404, "text/plain", e.as_bytes(), &[])?,
                    },
                    _ => respond(&mut out, 401, "text/plain", b"pair with the game first", &[])?,
                },
                ("POST", "/import") => {
                    if !self.authorized(&request) || request.header("origin").is_some_and(|_| !same_origin(&request)) {
                        respond(&mut out, 401, "text/plain", b"pair with the game first", &[])?;
                    } else {
                        let hint = request.param("name").unwrap_or("mod").to_string();
                        match crate::files::import(&request.body, &hint) {
                            Ok(name) => respond(&mut out, 200, "application/json", format!("{{\"mod\":\"{name}\"}}").as_bytes(), &[])?,
                            Err(e) => respond(&mut out, 400, "text/plain; charset=utf-8", e.as_bytes(), &[])?,
                        }
                    }
                }
                // Whether a code is the one (the page, to tell a game away
                // from a changed code).
                ("GET", "/check") => {
                    if self.authorized(&request) {
                        respond(&mut out, 200, "text/plain", b"ok", &[])?;
                    } else {
                        respond(&mut out, 401, "text/plain", b"wrong code", &[])?;
                    }
                }
                ("GET", "/natives.d.luau") => {
                    if self.authorized(&request) {
                        let defs = format!("{}{}", crate::world::definitions(), crate::natives::DEFINITIONS);
                        respond(&mut out, 200, "text/plain; charset=utf-8", defs.as_bytes(), &[])?;
                    } else {
                        respond(&mut out, 401, "text/plain", b"pair with the game first", &[])?;
                    }
                }
                ("GET" | "HEAD", path) => match assets::find(path) {
                    Some(file) if request.header("if-none-match") == Some(file.etag) => {
                        respond(&mut out, 304, file.mime, &[], &[("ETag", file.etag), ("Cache-Control", "no-cache")])?;
                    }
                    Some(file) => {
                        let gzip = request.header("accept-encoding").is_some_and(|e| e.contains("gzip"));
                        let (body, encoding) = match (gzip, file.gzip) {
                            (true, Some(gz)) => (gz, Some("gzip")),
                            _ => (file.body, None),
                        };
                        let mut headers = vec![("Cache-Control", "no-cache"), ("ETag", file.etag), ("Vary", "Accept-Encoding")];
                        if let Some(e) = encoding {
                            headers.push(("Content-Encoding", e));
                        }
                        let body = if request.method == "HEAD" { &[][..] } else { body };
                        respond(&mut out, 200, file.mime, body, &headers)?;
                    }
                    None => respond(&mut out, 404, "text/plain", b"not found", &[])?,
                },
                _ => respond(&mut out, 405, "text/plain", b"method not allowed", &[])?,
            }
            if close {
                return Ok(());
            }
        }
    }

    /// The token is right, and not guessed: after 10 wrong ones in a minute
    /// no token is taken for that minute.
    fn authorized(&self, request: &Request) -> bool {
        let given = request
            .param("token")
            .map(str::to_string)
            .or_else(|| request.header("authorization").and_then(|a| a.strip_prefix("Bearer ")).map(str::to_string));
        let Some(given) = given else { return false };
        let given: String = given.chars().filter(|c| *c != '-' && *c != ' ').collect::<String>().to_ascii_uppercase();
        let ok = given.len() == self.token.len()
            && given.bytes().zip(self.token.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
        if !ok {
            // A wrong code costs this connection a second: guessing 40 bits
            // takes ages, and a page elsewhere sending wrong ones locks no
            // one out (a limit for everyone would).
            std::thread::sleep(Duration::from_secs(1));
        }
        ok
    }
}

/// A WebSocket's Origin is the page's own (same host and port).
fn same_origin(request: &Request) -> bool {
    match (request.header("origin"), request.header("host")) {
        (Some(origin), Some(host)) => origin.split_once("://").is_some_and(|(_, rest)| rest.eq_ignore_ascii_case(host)),
        // Not a browser.
        (None, _) => true,
        _ => false,
    }
}

/// A connection read with a deadline: a request's head comes whole within
/// [`HEAD_TIME`] of its first byte (one byte now and then would otherwise
/// hold a connection, and the few there are, for ever).
struct Timed {
    stream: TcpStream,
    until: Option<Instant>,
}

/// How long a connection may wait for its next request, and take to send
/// one's head.
const IDLE: Duration = Duration::from_secs(60);
const HEAD_TIME: Duration = Duration::from_secs(10);

impl Read for Timed {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let wait = match self.until {
            Some(until) => match until.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) {
                Some(left) => left.min(IDLE),
                None => return Err(std::io::ErrorKind::TimedOut.into()),
            },
            None => IDLE,
        };
        self.stream.set_read_timeout(Some(wait))?;
        self.stream.read(buf)
    }
}

fn read_request(reader: &mut BufReader<Timed>) -> std::io::Result<Option<Request>> {
    // Idle until the request starts; then its head within the deadline.
    reader.get_mut().until = None;
    if reader.fill_buf()?.is_empty() {
        return Ok(None);
    }
    reader.get_mut().until = Some(Instant::now() + HEAD_TIME);
    let head = read_head(reader);
    reader.get_mut().until = None;
    let head = match head? {
        Some(head) => head,
        None => return Ok(None),
    };
    parse_request(&head)
}

fn read_head(reader: &mut BufReader<Timed>) -> std::io::Result<Option<Vec<u8>>> {
    let mut head = Vec::new();
    loop {
        let start = head.len();
        // Never more than the limit, even in one line.
        let left = (MAX_HEAD + 1).saturating_sub(head.len()) as u64;
        if reader.by_ref().take(left).read_until(b'\n', &mut head)? == 0 {
            return Ok(None);
        }
        if head.len() > MAX_HEAD {
            return Err(std::io::Error::other("request head too large"));
        }
        if head[start..] == *b"\r\n" || head[start..] == *b"\n" {
            return Ok(Some(head));
        }
    }
}

fn parse_request(head: &[u8]) -> std::io::Result<Option<Request>> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut parsed = httparse::Request::new(&mut headers);
    if !parsed.parse(head).map_err(std::io::Error::other)?.is_complete() {
        return Err(std::io::Error::other("incomplete request"));
    }
    let target = parsed.path.unwrap_or("/").to_string();
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let query = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect();
    let headers: Vec<(String, String)> = parsed
        .headers
        .iter()
        .map(|h| (h.name.to_string(), String::from_utf8_lossy(h.value).into_owned()))
        .collect();
    let mut request = Request {
        method: parsed.method.unwrap_or("GET").to_string(),
        path: decode(path),
        query,
        headers,
        body: Vec::new(),
        length: 0,
    };
    request.length = request.header("content-length").and_then(|l| l.trim().parse().ok()).unwrap_or(0);
    if request.length > MAX_BODY {
        return Err(std::io::Error::other("request body too large"));
    }
    Ok(Some(request))
}

/// Percent-decoding (and `+` as a space in queries).
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    None => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn respond(out: &mut TcpStream, status: u16, mime: &str, body: &[u8], headers: &[(&str, &str)]) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nX-Content-Type-Options: nosniff\r\n",
        body.len()
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    out.write_all(head.as_bytes())?;
    out.write_all(body)?;
    out.flush()
}

pub(crate) type Socket = tungstenite::WebSocket<TcpStream>;

/// Answers a WebSocket handshake, then speaks WebSocket on the stream.
fn accept(mut out: TcpStream, request: &Request) -> std::io::Result<Socket> {
    let key = request.header("sec-websocket-key").ok_or_else(|| std::io::Error::other("no Sec-WebSocket-Key"))?;
    let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
    write!(
        out,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    )?;
    out.flush()?;
    // Short reads let a session send while it waits for messages; a client
    // that stops reading does not hold the thread for ever.
    out.set_read_timeout(Some(Duration::from_millis(50)))?;
    out.set_write_timeout(Some(Duration::from_secs(10)))?;
    let config = tungstenite::protocol::WebSocketConfig::default().max_message_size(Some(MAX_BODY));
    Ok(tungstenite::WebSocket::from_raw_socket(out, tungstenite::protocol::Role::Server, Some(config)))
}

/// A message, or none yet (the read timed out), or the end. Other frames
/// (a pong) come as an empty message: the client is still there.
pub(crate) fn receive(ws: &mut Socket) -> Result<Option<String>, ()> {
    match ws.read() {
        Ok(tungstenite::Message::Text(t)) => Ok(Some(t.to_string())),
        Ok(tungstenite::Message::Close(_)) => Err(()),
        Ok(_) => Ok(Some(String::new())),
        Err(tungstenite::Error::Io(e))
            if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) =>
        {
            Ok(None)
        }
        Err(_) => Err(()),
    }
}

pub(crate) fn send(ws: &mut Socket, value: &serde_json::Value) -> Result<(), ()> {
    ws.send(tungstenite::Message::text(value.to_string())).map_err(|_| ())
}
