// The transport-level contract of a serving, end to end through `serve` and
// `dispatch_message`: id echo, notifications, malformed input, batches,
// cancellation, end of input, and `notifications/tools/list_changed`.
// Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions and #the-work-loop.
use super::*;

fn fresh_dir(tag: &str) -> std::path::PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "jazyk-mcp-dispatch-{}-{}-{}",
        tag,
        crate::sys::process_id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    crate::vfs::remove_dir_all(&dir).ok();
    crate::vfs::create_dir_all(dir.join("docs")).unwrap();
    crate::vfs::create_dir_all(dir.join(".jazyk")).unwrap();
    crate::vfs::write(
        dir.join(".jazyk/jazyk.toml"),
        "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
    )
    .unwrap();
    dir
}

fn chat_serving(dir: &std::path::Path) -> McpServer {
    let project = crate::document_sets::Workspace::load(dir)
        .unwrap()
        .files_project();
    let out = project.out.clone();
    McpServer::with_bridge(
        project,
        out,
        vec!["chat".to_string()],
        false,
        BridgeFlags {
            ephemeral: true,
            ..Default::default()
        },
    )
}

// Every line the serving wrote, parsed.
fn served(serving: &McpServer, input: &[u8]) -> Vec<Value> {
    let mut out: Vec<u8> = Vec::new();
    serving.serve(std::io::Cursor::new(input.to_vec()), &mut out);
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

// A writer the test reads while the serving still runs: each flushed line arrives
// on the channel.
struct LineSink {
    buf: Vec<u8>,
    tx: std::sync::mpsc::Sender<Value>,
}

impl std::io::Write for LineSink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(bytes);
        while let Some(end) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=end).collect();
            if let Ok(v) = serde_json::from_slice::<Value>(&line) {
                let _ = self.tx.send(v);
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// A response carries the request's id exactly as sent: a string stays a string, a
// number a number (zero and negatives included), and an error echoes it too.
#[test]
fn responses_echo_string_and_number_ids_as_sent() {
    let dir = fresh_dir("ids");
    let serving = chat_serving(&dir);
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":\"abc\",\"method\":\"ping\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":\"7\",\"method\":\"ping\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"ping\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":-3,\"method\":\"ping\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":\"x-1\",\"method\":\"no/such/method\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":\"no-method\"}\n",
    );
    let replies = served(&serving, input.as_bytes());
    let ids: Vec<Value> = replies.iter().map(|r| r["id"].clone()).collect();
    assert_eq!(
        ids,
        vec![
            json!("abc"),
            json!("7"),
            json!(7),
            json!(0),
            json!(-3),
            json!("x-1"),
            json!("no-method")
        ],
        "{replies:?}"
    );
    for r in &replies[..5] {
        assert_eq!(r["result"], json!({}), "{r}");
        assert_eq!(r["jsonrpc"], "2.0");
    }
    assert_eq!(replies[5]["error"]["code"], -32601, "{}", replies[5]);
    assert_eq!(replies[6]["error"]["code"], -32600, "{}", replies[6]);
    crate::vfs::remove_dir_all(dir).ok();
}

// A notification gets no line, whatever its method (known, unknown, or a
// cancellation naming nothing running); the request after it is answered.
#[test]
fn notifications_get_no_response_on_any_method() {
    let dir = fresh_dir("notifications");
    let serving = chat_serving(&dir);
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"no/such/notification\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{\"requestId\":99}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"ping\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n",
    );
    let replies = served(&serving, input.as_bytes());
    assert_eq!(
        replies,
        vec![json!({"jsonrpc":"2.0","id":1,"result":{}})],
        "{replies:?}"
    );
    crate::vfs::remove_dir_all(dir).ok();
}

// The loop survives what a careless client sends: a non-UTF-8 line is decoded
// lossily and answered as a parse error instead of ending the serving, blank and
// whitespace lines are skipped, a CRLF line parses, and the last line answers even
// without its newline.
#[test]
fn malformed_lines_answer_parse_errors_and_the_loop_goes_on() {
    let dir = fresh_dir("malformed");
    let serving = chat_serving(&dir);
    let mut input: Vec<u8> = Vec::new();
    input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\r\n");
    input.extend_from_slice(b"\xff\xfe{\"id\":2}\n");
    input.extend_from_slice(b"   \t \n\n");
    input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ping\",}\n");
    input.extend_from_slice(b"\"just a string\"\n");
    input.extend_from_slice(b"null\n");
    input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"ping\"}");
    let replies = served(&serving, &input);
    assert_eq!(replies.len(), 6, "{replies:?}");
    assert_eq!(replies[0], json!({"jsonrpc":"2.0","id":1,"result":{}}));
    assert_eq!(replies[1]["error"]["code"], -32700, "{}", replies[1]);
    assert_eq!(replies[1]["id"], Value::Null);
    assert_eq!(replies[2]["error"]["code"], -32700, "{}", replies[2]);
    assert_eq!(replies[3]["error"]["code"], -32600, "{}", replies[3]);
    assert_eq!(replies[4]["error"]["code"], -32600, "{}", replies[4]);
    assert_eq!(replies[5], json!({"jsonrpc":"2.0","id":4,"result":{}}));
    crate::vfs::remove_dir_all(dir).ok();
}

// A batch answers one array in request order: each request's reply with its own id,
// an invalid entry (a non-object, or an object without a method) its own error, and
// nothing for its notifications. A batch of notifications answers no line.
#[test]
fn batches_answer_in_order_with_per_entry_errors() {
    let dir = fresh_dir("batches");
    let serving = chat_serving(&dir);
    let batch = json!([
        {"jsonrpc":"2.0","id":"a","method":"ping"},
        {"jsonrpc":"2.0","method":"notifications/initialized"},
        5,
        {"jsonrpc":"2.0","id":"b"},
        {"jsonrpc":"2.0","id":2,"method":"no/such/method"},
        {"jsonrpc":"2.0","id":3,"method":"ping"}
    ]);
    let reply = serving.dispatch_message(&batch).unwrap();
    let items = reply.as_array().unwrap();
    assert_eq!(items.len(), 5, "{reply}");
    assert_eq!(items[0], json!({"jsonrpc":"2.0","id":"a","result":{}}));
    assert_eq!(items[1]["error"]["code"], -32600);
    assert_eq!(items[1]["id"], Value::Null);
    assert_eq!(items[2]["error"]["code"], -32600);
    assert_eq!(items[2]["id"], "b");
    assert_eq!(items[3]["error"]["code"], -32601);
    assert_eq!(items[3]["id"], 2);
    assert_eq!(items[4], json!({"jsonrpc":"2.0","id":3,"result":{}}));
    let notes = json!([
        {"jsonrpc":"2.0","method":"notifications/initialized"},
        {"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}
    ]);
    assert_eq!(serving.dispatch_message(&notes), None);
    // Over the stdio loop the same batch is one line holding the array.
    let line = format!("{}\n{}\n", batch, notes);
    let replies = served(&serving, line.as_bytes());
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_eq!(replies[0].as_array().unwrap().len(), 5);
    crate::vfs::remove_dir_all(dir).ok();
}

// A cancellation names one id by value and type: it drops the response of the
// request carrying that id (a number 7 is not the string "7"), and the reader
// notes one sent inside a batch too.
#[test]
fn a_cancellation_drops_only_the_response_of_the_id_it_names() {
    let dir = fresh_dir("cancel-ids");
    let serving = chat_serving(&dir);
    serving
        .interrupts
        .note(&json!({"jsonrpc":"2.0","method":"notifications/cancelled",
        "params":{"requestId":"7"}}));
    assert_eq!(
        serving.dispatch(&json!({"jsonrpc":"2.0","id":7,"method":"ping"})),
        Some(json!({"jsonrpc":"2.0","id":7,"result":{}}))
    );
    assert_eq!(
        serving.dispatch(&json!({"jsonrpc":"2.0","id":"7","method":"ping"})),
        None,
        "the cancelled call answered"
    );
    serving.interrupts.note(&json!([
        {"jsonrpc":"2.0","method":"ping"},
        {"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":8}},
        {"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}
    ]));
    assert_eq!(
        serving.dispatch(&json!({"jsonrpc":"2.0","id":8,"method":"ping"})),
        None
    );
    assert_eq!(
        serving.interrupts.cancelled.lock().unwrap().len(),
        0,
        "an answered cancellation leaves no stale entry"
    );
    crate::vfs::remove_dir_all(dir).ok();
}

// A running await_changes stops at the cancellation of its string id while the
// input stays open: the cancelled call answers nothing, and the ping behind it is
// the first line out. End of input then ends the loop.
#[cfg(unix)]
#[test]
fn await_changes_with_a_string_id_stops_at_its_cancellation_before_end_of_input() {
    use std::io::Write as _;
    let dir = fresh_dir("await-string");
    let project = crate::document_sets::Workspace::load(&dir)
        .unwrap()
        .files_project();
    let out_dir = project.out.clone();
    let serving = std::sync::Arc::new(McpServer::new(
        project,
        out_dir,
        vec!["graph".into()],
        false,
    ));
    let (reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    let runner = {
        let serving = serving.clone();
        std::thread::spawn(move || {
            serving.serve(
                std::io::BufReader::new(reader),
                LineSink {
                    buf: Vec::new(),
                    tx,
                },
            )
        })
    };
    writer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":\"poll-1\",\"method\":\"tools/call\",\"params\":{\"name\":\"await_changes\",\"arguments\":{\"timeout_seconds\":600}}}\n")
        .unwrap();
    writer
        .write_all(b"[{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{\"requestId\":\"poll-1\"}}]\n")
        .unwrap();
    writer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":\"after\",\"method\":\"ping\"}\n")
        .unwrap();
    // A generous bound only so a regression fails instead of hanging the suite.
    let first = rx
        .recv_timeout(std::time::Duration::from_secs(120))
        .expect("the cancelled poll kept the serving waiting");
    assert_eq!(first, json!({"jsonrpc":"2.0","id":"after","result":{}}));
    assert!(
        !serving
            .interrupts
            .eof
            .load(std::sync::atomic::Ordering::SeqCst),
        "the poll stopped on end of input, not on its cancellation"
    );
    drop(writer);
    runner.join().unwrap();
    assert!(serving
        .interrupts
        .eof
        .load(std::sync::atomic::Ordering::SeqCst));
    assert!(rx.try_recv().is_err(), "a line after the ping");
    crate::vfs::remove_dir_all(dir).ok();
}

// End of input ends the loop with every request before it answered, in order, and
// the serving records it so a call still running stops.
#[test]
fn end_of_input_ends_the_loop_after_answering_what_came_before() {
    let dir = fresh_dir("eof");
    let serving = chat_serving(&dir);
    let input: String = (1..=20)
        .map(|n| format!("{{\"jsonrpc\":\"2.0\",\"id\":{n},\"method\":\"ping\"}}\n"))
        .collect();
    let replies = served(&serving, input.as_bytes());
    assert_eq!(
        replies.iter().map(|r| r["id"].clone()).collect::<Vec<_>>(),
        (1..=20).map(|n| json!(n)).collect::<Vec<_>>()
    );
    assert!(serving
        .interrupts
        .eof
        .load(std::sync::atomic::Ordering::SeqCst));
    // An empty input answers nothing and still ends.
    let quiet = chat_serving(&dir);
    assert!(served(&quiet, b"").is_empty());
    crate::vfs::remove_dir_all(dir).ok();
}

// No announcement before the client listed: a move of the list before any
// `tools/list` stays silent. A `tools/list` inside a batch sets the baseline like a
// lone one, and the next move announces once.
#[test]
fn list_changed_waits_for_a_listing_and_counts_one_inside_a_batch() {
    let dir = fresh_dir("list-changed");
    crate::vfs::write(dir.join("docs/a.md"), "# A\n\nNavigation index.\n").unwrap();
    let project = crate::document_sets::Workspace::load(&dir)
        .unwrap()
        .resolve(Some("ds_main"))
        .unwrap();
    let out = project.out.clone();
    let mut control = crate::control::Control::load(&project, &out);
    control.compile = "auto".into();
    control.save(&out);
    let make = || {
        McpServer::with_bridge(
            project.clone(),
            out.clone(),
            vec!["compile".into()],
            true,
            BridgeFlags {
                ephemeral: true,
                ..Default::default()
            },
        )
    };
    let call = |id: &str, name: &str| {
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":{}}})
            .to_string()
            + "\n"
    };
    let shape = |lines: &[Value]| -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                if l.is_array() {
                    "batch".to_string()
                } else if let Some(id) = l["id"].as_str() {
                    id.to_string()
                } else {
                    l["method"].as_str().unwrap_or("?").to_string()
                }
            })
            .collect()
    };
    // Never listed: begin and abandon move the list without a word.
    let silent = [
        call("begin", "begin_goals"),
        call("abandon", "abandon_goals"),
    ]
    .concat();
    let lines = served(&make(), silent.as_bytes());
    assert_eq!(shape(&lines), ["begin", "abandon"], "{lines:?}");
    assert_ne!(lines[0]["result"]["isError"], true, "{}", lines[0]);
    // Listed inside a batch: the batch itself announces nothing, the move after it does.
    let batch = json!([
        {"jsonrpc":"2.0","id":"list","method":"tools/list"},
        {"jsonrpc":"2.0","id":"ping","method":"ping"}
    ])
    .to_string()
        + "\n";
    let listed = [
        batch,
        call("begin", "begin_goals"),
        call("abandon", "abandon_goals"),
    ]
    .concat();
    let lines = served(&make(), listed.as_bytes());
    assert_eq!(
        shape(&lines),
        [
            "batch",
            "begin",
            "notifications/tools/list_changed",
            "abandon",
            "notifications/tools/list_changed"
        ],
        "{lines:?}"
    );
    crate::vfs::remove_dir_all(dir).ok();
}
