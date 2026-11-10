use super::*;

impl McpServer {
    // The stdio transport: line-delimited JSON-RPC until stdin ends.
    pub fn run(&self) {
        self.serve(std::io::BufReader::new(std::io::stdin()), std::io::stdout());
        self.finish();
    }

    // The stdio loop over any reader and writer. A reader thread takes the lines so a
    // cancellation or the end of input reaches a call that is still running.
    // Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
    pub fn serve<R: BufRead + Send + 'static, W: Write>(&self, mut input: R, mut out: W) {
        self.notifies
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let interrupts = self.interrupts.clone();
        std::thread::spawn(move || loop {
            // Bytes, not `lines()`: one non-UTF-8 byte must not end the serving as if
            // the agent had hung up, and a line is decoded lossily instead.
            let mut bytes: Vec<u8> = Vec::new();
            match input.read_until(b'\n', &mut bytes) {
                Ok(0) | Err(_) => {
                    interrupts
                        .eof
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                    break;
                }
                Ok(_) => {}
            }
            if let Ok(message) = serde_json::from_slice::<Value>(&bytes) {
                interrupts.note(&message);
            }
            if tx.send(bytes).is_err() {
                break;
            }
        });
        for bytes in rx {
            let line = String::from_utf8_lossy(&bytes);
            if line.trim().is_empty() {
                continue;
            }
            let message = serde_json::from_str::<Value>(&line);
            let resp = match &message {
                Ok(message) => self.dispatch_message(message),
                Err(e) => Some(json!({"jsonrpc": "2.0", "id": null,
                    "error": {"code": -32700, "message": format!("parse error: {}", e)}})),
            };
            if let Some(resp) = resp {
                writeln!(out, "{}", resp).ok();
                out.flush().ok();
            }
            if let Some(note) = message.ok().and_then(|m| self.tools_list_changed(&m)) {
                writeln!(out, "{}", note).ok();
                out.flush().ok();
            }
        }
    }

    // What initialize advertises: `listChanged` only where the transport can deliver
    // the notification. Mirrors docs/frontends/mcp.md#the-work-loop.
    pub(super) fn capabilities(&self) -> Value {
        if self.notifies.load(std::sync::atomic::Ordering::SeqCst) {
            json!({"tools": {"listChanged": true}})
        } else {
            json!({"tools": {}})
        }
    }

    // The tool names `tools/list` would answer now.
    fn listed_names(&self) -> Option<Vec<String>> {
        let list = self.handle("tools/list", &json!({})).ok()?;
        Some(
            list["tools"]
                .as_array()?
                .iter()
                .filter_map(|t| t["name"].as_str().map(String::from))
                .collect(),
        )
    }

    // After one message: a client's `tools/list` sets the baseline; any other message
    // that moved the list since then answers the notification, and the new names
    // become the baseline. Mirrors docs/frontends/mcp.md#the-work-loop.
    fn tools_list_changed(&self, message: &Value) -> Option<Value> {
        fn lists(m: &Value) -> bool {
            match m {
                Value::Array(items) => items.iter().any(lists),
                m => m["method"] == "tools/list" && !m["id"].is_null(),
            }
        }
        let listing = lists(message);
        if !listing && self.listed.lock().unwrap().is_none() {
            return None;
        }
        let now = self.listed_names()?;
        let mut listed = self.listed.lock().unwrap();
        let moved = !listing && listed.as_ref() != Some(&now);
        *listed = Some(now);
        moved.then(|| json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"}))
    }

    // One parsed message, whatever the transport: a request answers its response, a
    // notification nothing, a batch the array of its responses (None when it held
    // notifications only), and anything else an invalid request.
    pub fn dispatch_message(&self, message: &Value) -> Option<Value> {
        let invalid = |what: &str| {
            json!({"jsonrpc": "2.0", "id": null,
            "error": {"code": -32600, "message": format!("invalid request: {}", what)}})
        };
        match message {
            Value::Array(items) if items.is_empty() => Some(invalid("an empty batch")),
            Value::Array(items) => {
                let replies: Vec<Value> = items
                    .iter()
                    .filter_map(|m| match m {
                        Value::Object(_) => self.dispatch(m),
                        _ => Some(invalid("a batch entry is not an object")),
                    })
                    .collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            Value::Object(_) => self.dispatch(message),
            _ => Some(invalid("not a JSON-RPC object")),
        }
    }

    // One JSON-RPC message in, its response out, whatever the transport carried it
    // (the stdio loop above, or the HTTP serving in acp/http.rs). A notification (no
    // id) gets no response, and neither does a request the client cancelled while it
    // ran. Mirrors docs/frontends/mcp.md#mcp-over-http.
    pub fn dispatch(&self, req: &Value) -> Option<Value> {
        let id = req["id"].clone();
        if id.is_null() {
            return None;
        }
        let Some(method) = req["method"].as_str() else {
            return Some(json!({"jsonrpc": "2.0", "id": id,
                "error": {"code": -32600, "message": "invalid request: method is required"}}));
        };
        let key = id.to_string();
        *self.interrupts.current.lock().unwrap() = Some(key.clone());
        let result = self.handle(method, &req["params"]);
        *self.interrupts.current.lock().unwrap() = None;
        if self.interrupts.cancelled.lock().unwrap().remove(&key) {
            return None;
        }
        Some(match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, msg)) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
            }
        })
    }

    // The serving's end, on any transport: an ephemeral serving with an open batch
    // runs the implicit finish, and the transcript closes.
    pub fn finish(&self) {
        for child in self.children.lock().unwrap().values() {
            child.finish();
        }
        if self.cancelled() {
            self.cancel_owned();
        } else if self.bridge.ephemeral {
            self.eof_finish();
        }
        self.trace
            .finish_transcript("done", &json!({"modes": self.modes}));
    }

    /// Cancellation discards staged work, including children, without EOF commit.
    pub(crate) fn cancel_owned(&self) {
        for child in self.children.lock().unwrap().values() {
            child.cancel_owned();
        }
        if let Some(batch) = self.open.lock().unwrap().take() {
            if let Some(claim) =
                crate::control::owned_lease(&self.out, &batch.id, &self.worker_id())
            {
                let _ = crate::control::release_owned_lease(&claim);
            }
        }
        if let Some(target) = self.ledger_target.lock().unwrap().take() {
            self.release_own_ledger_claim(&target);
        }
        if let Some(worker) = self.worker.lock().unwrap().as_mut() {
            worker.refresh(Some(""));
        }
    }

    // A transport-level note in the transcript (a refused call, a bind), under the
    // serving's label rather than a batch's.
    pub fn note(&self, text: &str) {
        self.trace.event(crate::session::TraceEvent::Note {
            label: format!("mcp {}", self.modes.join(",")),
            text: text.to_string(),
            verbose: false,
        });
    }

    pub(super) fn caller(&self, task: &str, target: &str) -> crate::feedback::Caller {
        crate::feedback::Caller {
            source: "mcp".into(),
            task: task.into(),
            target: target.into(),
            client: self.client.lock().unwrap().clone(),
            ..Default::default()
        }
    }
}
