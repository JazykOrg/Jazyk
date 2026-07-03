// Content-Length framed JSON-RPC over stdio, used by the LSP server. (The MCP server
// speaks line-delimited JSON-RPC and has its own loop.)
use serde_json::Value;
use std::io::{BufRead, Write};

// Read one Content-Length framed JSON-RPC message from `r`. Returns None on EOF.
pub fn read_message<R: BufRead>(r: &mut R) -> Option<Value> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = r.read_line(&mut line).ok()?;
        if n == 0 {
            return None; // EOF
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break; // end of headers
        }
        if let Some(rest) = trimmed.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = rest.trim().parse::<usize>().ok();
        }
    }
    let len = content_length?;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

// Write one Content-Length framed JSON-RPC message to `w`.
pub fn write_message<W: Write>(w: &mut W, msg: &Value) {
    let body = serde_json::to_string(msg).unwrap_or_else(|_| "{}".to_string());
    let _ = write!(w, "Content-Length: {}\r\n\r\n{}", body.as_bytes().len(), body);
    let _ = w.flush();
}
