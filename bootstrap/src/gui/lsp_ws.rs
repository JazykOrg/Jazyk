// The language server over WebSocket, for the GUI's editor. One connection is one
// session with its own open-document overlay; the wire is one JSON-RPC message per
// text frame, no Content-Length framing (what monaco-languageclient speaks).
// A store.generation event republishes diagnostics on every session, the same refresh
// the stdio server runs. Mirrors docs/frontends/gui.md#editor.
use super::state::SharedState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;

// Frames waiting for the language server thread. A frame arriving at a full queue
// is dropped and logged once per connection (docs/frontends/gui.md#serving).
const INBOUND_QUEUE: usize = 1024;

enum SessionEvent {
    Client(Value),
    StoreChanged,
    Closed,
}

// An io::Write sink that unframes the Content-Length messages the LSP core writes and
// forwards each JSON body as its own WebSocket text frame.
struct FrameSink {
    buf: Vec<u8>,
    tx: tokio::sync::mpsc::UnboundedSender<String>,
}

impl FrameSink {
    fn drain(&mut self) {
        loop {
            let Some(header_end) = self.buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                return;
            };
            let header = String::from_utf8_lossy(&self.buf[..header_end]).to_string();
            let Some(len) = header
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|r| r.trim().to_string())
                })
                .and_then(|v| v.parse::<usize>().ok())
            else {
                self.buf.drain(..header_end + 4);
                continue;
            };
            let body_start = header_end + 4;
            if self.buf.len() < body_start + len {
                return; // body incomplete, wait for more writes
            }
            let body = String::from_utf8_lossy(&self.buf[body_start..body_start + len]).to_string();
            self.buf.drain(..body_start + len);
            let _ = self.tx.send(body);
        }
    }
}

impl std::io::Write for FrameSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(buf);
        self.drain();
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub async fn ws(State(st): State<SharedState>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| session(st, socket))
}

async fn session(st: SharedState, socket: WebSocket) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (in_tx, in_rx) = std::sync::mpsc::sync_channel::<SessionEvent>(INBOUND_QUEUE);

    let writer = tokio::spawn(async move {
        while let Some(s) = out_rx.recv().await {
            if ws_tx.send(Message::Text(s.into())).await.is_err() {
                break;
            }
        }
    });

    // A committed build repaints every open document without client activity.
    let mut sub = st.events.subscribe();
    let gen_tx = in_tx.clone();
    let gen_task = tokio::spawn(async move {
        loop {
            match sub.recv().await {
                Ok(ev) => {
                    if ev["type"] != "store.generation" {
                        continue;
                    }
                    // A full queue drops the nudge: the next one repaints the same
                    // documents, and the thread refreshes on every client message.
                    match gen_tx.try_send(SessionEvent::StoreChanged) {
                        Ok(()) | Err(std::sync::mpsc::TrySendError::Full(_)) => {}
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => break,
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });

    let (root, out_dir, gs) = (st.proj().root.clone(), st.out.clone(), st.gs().clone());
    let jobs_st = st.clone();
    let session = tokio::task::spawn_blocking(move || {
        // The editor's compile command is a GUI job: queued, cancellable, and visible
        // with every other job (docs/frontends/lsp.md#commands).
        let mut lsp = crate::lsp::Lsp::new(root, out_dir, gs).with_job_submitter(Box::new(
            move |set_id, config_hash| {
                let id = jobs_st.jobs.submit(
                    &jobs_st,
                    super::jobs::JobKind::CompileSets {
                        set_ids: vec![set_id.clone()],
                        config_hash,
                    },
                );
                serde_json::json!({"started": [set_id], "jobId": id})
            },
        ));
        let mut sink = FrameSink {
            buf: Vec::new(),
            tx: out_tx,
        };
        for ev in in_rx {
            match ev {
                SessionEvent::Client(v) => {
                    if !lsp.handle(v, &mut sink) {
                        break;
                    }
                }
                SessionEvent::StoreChanged => lsp.refresh(&mut sink),
                SessionEvent::Closed => break,
            }
        }
    });

    let mut dropped_once = false;
    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Text(t) => {
                if let Ok(v) = serde_json::from_str::<Value>(&t) {
                    match in_tx.try_send(SessionEvent::Client(v)) {
                        Ok(()) => {}
                        Err(std::sync::mpsc::TrySendError::Full(_)) => {
                            if !dropped_once {
                                dropped_once = true;
                                eprintln!(
                                    "[jazyk] warning: the editor's language server is {} frames behind; dropping frames until it catches up",
                                    INBOUND_QUEUE
                                );
                            }
                        }
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => break,
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    // The close marker may find the queue full: dropping every sender closes the
    // channel too, so the thread ends once it drains. Both senders go first, or
    // waiting on the thread would wait on ourselves.
    let _ = in_tx.try_send(SessionEvent::Closed);
    drop(in_tx);
    gen_task.abort();
    let _ = session.await;
    writer.abort();
}
