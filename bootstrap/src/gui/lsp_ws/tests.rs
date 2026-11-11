// The editor's language server over WebSocket: the Content-Length unframing toward
// the socket, the inbound queue, the upgrade's origin and token checks, and a whole
// session over a real loopback socket. Mirrors docs/frontends/lsp.md and
// docs/frontends/gui.md#serving.
use super::*;
use std::io::{Read, Write};

fn sink() -> (FrameSink, tokio::sync::mpsc::UnboundedReceiver<String>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (
        FrameSink {
            buf: Vec::new(),
            tx,
        },
        rx,
    )
}

fn framed(body: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
}

fn drained(rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>) -> Vec<String> {
    let mut v = Vec::new();
    while let Ok(s) = rx.try_recv() {
        v.push(s);
    }
    v
}

// Two messages in one write leave as two frames, in order; a message written a
// byte at a time leaves once, when its last byte lands.
#[test]
fn the_sink_unframes_whole_split_and_coalesced_messages() {
    let (mut s, mut rx) = sink();
    let a = r#"{"jsonrpc":"2.0","id":1,"result":null}"#;
    let b = r#"{"jsonrpc":"2.0","method":"window/logMessage","params":{"message":"é 😀"}}"#;
    s.write_all(format!("{}{}", framed(a), framed(b)).as_bytes())
        .unwrap();
    assert_eq!(drained(&mut rx), vec![a.to_string(), b.to_string()]);
    let whole = framed(b);
    let bytes = whole.as_bytes();
    for (i, byte) in bytes.iter().enumerate() {
        s.write_all(&[*byte]).unwrap();
        let out = drained(&mut rx);
        if i + 1 < bytes.len() {
            assert!(out.is_empty(), "a frame left early at byte {}", i);
        } else {
            assert_eq!(out, vec![b.to_string()]);
        }
    }
    assert!(s.buf.is_empty());
}

// The length counts bytes, not characters; the header name is case-insensitive and
// other headers ride along; a header without a length is skipped and the message
// after it still leaves.
#[test]
fn the_sink_reads_byte_lengths_any_header_case_and_skips_lengthless_headers() {
    let (mut s, mut rx) = sink();
    let body = r#"{"text":"𝔸 ž 😀"}"#;
    let wire = format!(
        "content-type: application/vscode-jsonrpc; charset=utf-8\r\ncontent-LENGTH:  {}\r\n\r\n{}",
        body.len(),
        body
    );
    s.write_all(wire.as_bytes()).unwrap();
    assert_eq!(drained(&mut rx), vec![body.to_string()]);
    s.write_all(b"X-Nothing: 1\r\n\r\n").unwrap();
    s.write_all(framed("{}").as_bytes()).unwrap();
    assert_eq!(drained(&mut rx), vec!["{}".to_string()]);
    s.flush().unwrap();
}

// The inbound queue: JSON frames queue until it is full, a frame at a full queue is
// dropped (the first drop flags the connection), room frees as the thread drains,
// a frame that is not JSON is ignored, and a gone thread ends the reading.
#[test]
fn inbound_frames_queue_drop_when_full_and_stop_when_the_thread_is_gone() {
    let (tx, rx) = std::sync::mpsc::sync_channel::<SessionEvent>(2);
    let mut dropped = false;
    assert_eq!(admit(&tx, r#"{"id":1}"#, &mut dropped), Admit::Queued);
    assert_eq!(admit(&tx, r#"{"id":2}"#, &mut dropped), Admit::Queued);
    assert!(!dropped);
    assert_eq!(admit(&tx, r#"{"id":3}"#, &mut dropped), Admit::Dropped);
    assert!(dropped, "the first drop flags the connection");
    assert_eq!(admit(&tx, r#"{"id":4}"#, &mut dropped), Admit::Dropped);
    assert_eq!(admit(&tx, "not json", &mut dropped), Admit::Ignored);
    let first = rx.recv().unwrap();
    assert!(matches!(first, SessionEvent::Client(v) if v["id"] == 1));
    assert_eq!(admit(&tx, r#"{"id":5}"#, &mut dropped), Admit::Queued);
    let ids: Vec<Value> = rx
        .try_iter()
        .map(|e| match e {
            SessionEvent::Client(v) => v["id"].clone(),
            _ => Value::Null,
        })
        .collect();
    assert_eq!(ids, vec![serde_json::json!(2), serde_json::json!(5)]);
    drop(rx);
    assert_eq!(admit(&tx, r#"{"id":6}"#, &mut dropped), Admit::Gone);
    // The real queue holds the documented number of frames.
    assert_eq!(INBOUND_QUEUE, 1024);
}

// ---- a minimal WebSocket client over a blocking loopback socket ----

struct Client {
    stream: std::net::TcpStream,
}

impl Client {
    // Open the upgrade; Ok(client) on 101, Err(status) otherwise.
    fn connect(port: u16, path: &str, headers: &[(&str, &str)]) -> Result<(Client, String), u16> {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        // A safety bound so a regression fails instead of hanging the suite.
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(60)))
            .unwrap();
        let mut req = format!(
            "GET {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n",
            path, port
        );
        for (k, v) in headers {
            req.push_str(&format!("{}: {}\r\n", k, v));
        }
        req.push_str("\r\n");
        stream.write_all(req.as_bytes()).unwrap();
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if stream.read(&mut byte).unwrap() == 0 {
                break;
            }
            head.push(byte[0]);
        }
        let head = String::from_utf8_lossy(&head).to_string();
        let status: u16 = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if status == 101 {
            Ok((Client { stream }, head))
        } else {
            Err(status)
        }
    }

    // One masked client frame.
    fn send(&mut self, opcode: u8, payload: &[u8]) {
        let mut frame = vec![0x80 | opcode];
        let len = payload.len();
        if len < 126 {
            frame.push(0x80 | len as u8);
        } else if len <= u16::MAX as usize {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
        let mask = [0x12u8, 0x34, 0x56, 0x78];
        frame.extend_from_slice(&mask);
        frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.stream.write_all(&frame).unwrap();
    }

    fn text(&mut self, s: &str) {
        self.send(0x1, s.as_bytes());
    }

    // One server frame: (opcode, payload), or None at end of stream.
    fn recv(&mut self) -> Option<(u8, Vec<u8>)> {
        let mut h = [0u8; 2];
        self.stream.read_exact(&mut h).ok()?;
        let opcode = h[0] & 0x0f;
        assert_eq!(h[1] & 0x80, 0, "a server frame is never masked");
        let len = match h[1] & 0x7f {
            126 => {
                let mut b = [0u8; 2];
                self.stream.read_exact(&mut b).ok()?;
                u16::from_be_bytes(b) as usize
            }
            127 => {
                let mut b = [0u8; 8];
                self.stream.read_exact(&mut b).ok()?;
                u64::from_be_bytes(b) as usize
            }
            n => n as usize,
        };
        let mut payload = vec![0u8; len];
        self.stream.read_exact(&mut payload).ok()?;
        Some((opcode, payload))
    }

    // The next text frame that answers `id`, skipping notifications.
    fn answer(&mut self, id: u64) -> Value {
        loop {
            let (opcode, payload) = self.recv().expect("the socket closed before the answer");
            if opcode != 0x1 {
                continue;
            }
            let v: Value = serde_json::from_slice(&payload).unwrap();
            if v["id"] == id {
                return v;
            }
        }
    }
}

async fn listen(st: SharedState) -> (u16, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = super::super::server::router_on(st, Some(port));
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (port, task)
}

// Who may open the editor's socket: the page's origin (and a dev server's loopback
// twin) or a client sending no origin, never a foreign one; with a session token,
// only a connection carrying it, as a query or a bearer header.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_upgrade_admits_the_page_and_local_tools_and_checks_the_token() {
    let page = "http://localhost:5173";
    let cases: Vec<(Option<&str>, &str, Option<&str>, Option<&str>, u16)> = vec![
        // (token, path, origin, bearer, expected status)
        (None, "/lsp", Some(page), None, 101),
        (None, "/lsp", Some("http://127.0.0.1:5173"), None, 101),
        (None, "/lsp", None, None, 101),
        (None, "/lsp", Some("https://evil.example"), None, 403),
        (None, "/lsp", Some("null"), None, 403),
        (Some("secret"), "/lsp", Some(page), None, 401),
        (Some("secret"), "/lsp?token=secret", Some(page), None, 101),
        (Some("secret"), "/lsp?token=wrong", Some(page), None, 401),
        (Some("secret"), "/lsp", Some(page), Some("secret"), 101),
        (Some("secret"), "/lsp", Some(page), Some("wrong"), 401),
        (Some("secret"), "/lsp?token=secret", None, None, 101),
        (
            Some("secret"),
            "/lsp?token=secret",
            Some("https://evil.example"),
            None,
            403,
        ),
    ];
    for token in [None, Some("secret")] {
        let st = super::super::server::tests::state(token, page);
        let root = st.root.clone();
        let (port, server) = listen(st).await;
        for (t, path, origin, bearer, want) in cases.iter().filter(|c| c.0 == token) {
            let (shown, origin, bearer, want) = (path.to_string(), *origin, *bearer, *want);
            let path = shown.clone();
            let got = tokio::task::spawn_blocking(move || {
                let bearer = bearer.map(|b| format!("Bearer {}", b));
                let mut headers: Vec<(&str, &str)> = Vec::new();
                if let Some(o) = origin {
                    headers.push(("Origin", o));
                }
                if let Some(b) = &bearer {
                    headers.push(("Authorization", b));
                }
                match Client::connect(port, &path, &headers) {
                    Ok((mut c, _)) => {
                        c.send(0x8, &[]);
                        101
                    }
                    Err(status) => status,
                }
            })
            .await
            .unwrap();
            assert_eq!(got, want, "{:?} {} {:?} {:?}", t, shown, origin, bearer);
        }
        server.abort();
        std::fs::remove_dir_all(&root).ok();
    }
}

// One session end to end: each JSON-RPC message is one text frame with no
// Content-Length framing, a frame that is not JSON and a binary frame are ignored
// without ending the session, a frame past the 16-bit length decodes, and a close
// ends the session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_speaks_one_message_per_text_frame_until_close() {
    let st = super::super::server::tests::state(None, "https://jazyk.dev");
    let root = st.root.clone();
    let (port, server) = listen(st).await;
    let uri = url::Url::from_file_path(root.join("docs/big.md"))
        .unwrap()
        .to_string();
    let outcome = tokio::task::spawn_blocking(move || {
        let (mut c, head) =
            Client::connect(port, "/lsp", &[("Origin", "https://jazyk.dev")]).unwrap();
        assert!(
            head.to_ascii_lowercase()
                .contains("sec-websocket-accept: s3pplmbitxaq9kygzzhzrbk+xoo="),
            "{}",
            head
        );
        c.text(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
        let init = c.answer(1);
        assert_eq!(
            init["result"]["capabilities"]["textDocumentSync"], 1,
            "{init}"
        );
        c.text("this is not json");
        c.send(0x2, b"\x00\x01binary");
        let text = "The cart holds items. ".repeat(4000);
        let open = serde_json::json!({"jsonrpc":"2.0","method":"textDocument/didOpen",
            "params":{"textDocument":{"uri":uri,"languageId":"markdown","version":1,"text":text}}});
        let open = open.to_string();
        assert!(open.len() > u16::MAX as usize);
        c.text(&open);
        c.text(&format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":0,"character":4}}}}}}"#,
            uri
        ));
        let hover = c.answer(2);
        assert!(hover.get("result").is_some(), "{hover}");
        assert!(hover.get("error").is_none(), "{hover}");
        c.send(0x8, &[0x03, 0xe8]);
        // The server ends the session: a close frame back, or the stream ends.
        loop {
            match c.recv() {
                None => break "ended",
                Some((0x8, _)) => break "closed",
                Some(_) => continue,
            }
        }
    })
    .await
    .unwrap();
    assert!(matches!(outcome, "ended" | "closed"), "{}", outcome);
    server.abort();
    std::fs::remove_dir_all(&root).ok();
}
