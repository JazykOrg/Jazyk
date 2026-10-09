// Content-Length framed JSON-RPC over stdio, used by the LSP server. (The MCP server
// speaks line-delimited JSON-RPC and has its own loop.)
use serde_json::Value;
use std::io::{BufRead, Write};

// The largest frame the reader accepts; a header past it is a broken peer, not a
// message, and allocating for it would abort the process.
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

// Read one Content-Length framed JSON-RPC message from `r`. Returns None when the
// stream is spent: EOF, a frame without a length, or a length over the cap. A frame
// whose body is not JSON is skipped and the next one read.
pub fn read_message<R: BufRead>(r: &mut R) -> Option<Value> {
    loop {
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
        if len > MAX_FRAME_BYTES {
            eprintln!(
                "[jazyk] jsonrpc: a {} byte frame exceeds the {} byte cap; closing the stream",
                len, MAX_FRAME_BYTES
            );
            return None;
        }
        let mut buf = vec![0u8; len];
        r.read_exact(&mut buf).ok()?;
        match serde_json::from_slice(&buf) {
            Ok(v) => return Some(v),
            Err(e) => eprintln!("[jazyk] jsonrpc: skipping a frame that is not JSON: {}", e),
        }
    }
}

// Write one Content-Length framed JSON-RPC message to `w`.
pub fn write_message<W: Write>(w: &mut W, msg: &Value) {
    let body = serde_json::to_string(msg).unwrap_or_else(|_| "{}".to_string());
    let _ = write!(
        w,
        "Content-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = w.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(body: &str) -> String {
        format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
    }

    #[test]
    fn a_frame_that_is_not_json_is_skipped_and_the_next_one_read() {
        let input = format!("{}{}", frame("this is not json"), frame(r#"{"id":1}"#));
        let mut r = std::io::BufReader::new(input.as_bytes());
        assert_eq!(read_message(&mut r), Some(serde_json::json!({"id": 1})));
        assert_eq!(read_message(&mut r), None);
    }

    #[test]
    fn a_length_over_the_cap_closes_the_stream_without_allocating() {
        let input = format!("Content-Length: {}\r\n\r\n{{}}", MAX_FRAME_BYTES + 1);
        let mut r = std::io::BufReader::new(input.as_bytes());
        assert_eq!(read_message(&mut r), None);
    }

    #[test]
    fn write_then_read_round_trips() {
        let mut buf = Vec::new();
        let msg = serde_json::json!({"jsonrpc": "2.0", "method": "x", "params": {"é": "ü"}});
        write_message(&mut buf, &msg);
        let mut r = std::io::BufReader::new(buf.as_slice());
        assert_eq!(read_message(&mut r), Some(msg));
    }
}
