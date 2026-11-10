use super::*;

// A stdio serving advertises listChanged and announces each move of the list the
// client already listed: begin_goals narrows it, abandon_goals restores it, and a
// message that moves nothing announces nothing. A serving outside the stdio loop
// advertises nothing it cannot deliver. Mirrors docs/frontends/mcp.md#the-work-loop.
#[test]
fn stdio_announces_tool_list_changes_after_the_first_listing() {
    let dir = bare_dir("list-changed");
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
    let direct = make().handle("initialize", &json!({})).unwrap();
    assert_eq!(direct["capabilities"]["tools"], json!({}), "{direct}");
    let request = |id: u64, method: &str, params: Value| {
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string() + "\n"
    };
    let input = [
        request(1, "initialize", json!({})),
        request(2, "ping", json!({})),
        request(3, "tools/list", json!({})),
        request(4, "ping", json!({})),
        request(
            5,
            "tools/call",
            json!({"name":"begin_goals","arguments":{}}),
        ),
        request(6, "ping", json!({})),
        request(
            7,
            "tools/call",
            json!({"name":"abandon_goals","arguments":{}}),
        ),
    ]
    .concat();
    let mut written: Vec<u8> = Vec::new();
    make().serve(std::io::Cursor::new(input.into_bytes()), &mut written);
    let lines: Vec<Value> = String::from_utf8(written)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let shape: Vec<String> = lines
        .iter()
        .map(|l| {
            l["id"]
                .as_u64()
                .map(|id| id.to_string())
                .unwrap_or_else(|| l["method"].as_str().unwrap_or("?").to_string())
        })
        .collect();
    assert_eq!(
        shape,
        [
            "1",
            "2",
            "3",
            "4",
            "5",
            "notifications/tools/list_changed",
            "6",
            "7",
            "notifications/tools/list_changed"
        ],
        "{lines:?}"
    );
    assert_eq!(
        lines[0]["result"]["capabilities"]["tools"]["listChanged"], true,
        "{}",
        lines[0]
    );
    assert_ne!(lines[4]["result"]["isError"], true, "{}", lines[4]);
    assert_eq!(
        lines[5],
        json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"})
    );
    crate::vfs::remove_dir_all(dir).ok();
}

// initialize echoes a supported requested version and answers its latest supported
// version for anything else, on the project serving and a set-scoped one alike.
#[test]
fn initialize_negotiates_the_protocol_version() {
    let dir = bare_dir("protocol-version");
    for serving in [
        workspace_chat(&dir, BridgeFlags::default()),
        chat_server(&dir),
    ] {
        let version = |requested: Value| {
            serving
                .handle("initialize", &json!({"protocolVersion": requested}))
                .unwrap()["protocolVersion"]
                .clone()
        };
        assert_eq!(version(json!("2025-03-26")), "2025-03-26");
        assert_eq!(version(json!("2024-11-05")), "2024-11-05");
        assert_eq!(version(json!("2099-01-01")), "2025-11-25");
        assert_eq!(version(json!("not a version")), "2025-11-25");
        assert_eq!(version(Value::Null), "2024-11-05");
    }
    crate::vfs::remove_dir_all(dir).ok();
}

// A refusal inside tools/call is an isError result naming the refusal; JSON-RPC
// errors stay for unknown tools, calls without a name, and unknown methods.
// Mirrors docs/frontends/mcp.md#toolsets.
#[test]
fn refusals_are_tool_results_and_unknown_tools_are_protocol_errors() {
    let dir = bare_dir("refusal-shape");
    let reader = McpServer::with_bridge(
        crate::document_sets::Workspace::load(&dir)
            .unwrap()
            .files_project(),
        dir.join(".jazyk/ds_main"),
        vec!["graph".into()],
        false,
        BridgeFlags::default(),
    );
    for (name, args) in [
        ("read_jazyk_skill", json!({})),
        ("read_jazyk_skill", json!({"name":"../../secret"})),
        ("read_document", json!({})),
    ] {
        let reply = reader
            .handle("tools/call", &json!({"name":name,"arguments":args}))
            .unwrap();
        assert_eq!(reply["isError"], true, "{name}: {reply}");
        let body: Value =
            serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["error"]["rule"], "refused", "{name}: {body}");
        assert!(body["error"]["message"]
            .as_str()
            .is_some_and(|m| !m.is_empty()));
    }
    // A tool the serving does not list is not served: a protocol error.
    for name in [
        "no_such_tool",
        "write_document",
        "compile_document_sets",
        "update_document_set",
    ] {
        let unknown = reader
            .handle("tools/call", &json!({"name":name,"arguments":{}}))
            .unwrap_err();
        assert_eq!(unknown.0, -32602);
        assert!(unknown.1.starts_with(UNKNOWN_TOOL), "{name}: {unknown:?}");
    }
    assert!(reader
        .handle("tools/call", &json!({"arguments":{}}))
        .is_err());
    assert_eq!(
        reader.handle("no/such/method", &json!({})).unwrap_err().0,
        -32601
    );
    crate::vfs::remove_dir_all(dir).ok();
}

// The stdio loop answers what it cannot dispatch instead of dropping it: a parse
// error, an invalid request, and a batch with the array of its responses.
// Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
#[test]
fn stdio_answers_parse_errors_invalid_requests_and_batches() {
    let dir = bare_dir("stdio-shapes");
    let serving = workspace_chat(
        &dir,
        BridgeFlags {
            ephemeral: true,
            ..Default::default()
        },
    );
    let input = concat!(
            "{not json\n",
            "\n",
            "[{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"},{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"},{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}]\n",
            "[{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}]\n",
            "[]\n",
            "3\n",
            "{\"jsonrpc\":\"2.0\",\"id\":4}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"ping\"}\n",
        );
    let mut out: Vec<u8> = Vec::new();
    serving.serve(std::io::Cursor::new(input.as_bytes().to_vec()), &mut out);
    let replies: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies.len(), 6, "{replies:?}");
    assert_eq!(replies[0]["error"]["code"], -32700);
    assert_eq!(replies[0]["id"], Value::Null);
    assert_eq!(
        replies[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].clone())
            .collect::<Vec<_>>(),
        vec![json!(1), json!(2)]
    );
    assert_eq!(replies[2]["error"]["code"], -32600);
    assert_eq!(replies[3]["error"]["code"], -32600);
    assert_eq!(replies[4]["error"]["code"], -32600);
    assert_eq!(replies[4]["id"], 4);
    assert_eq!(replies[5], json!({"jsonrpc":"2.0","id":5,"result":{}}));
    crate::vfs::remove_dir_all(dir).ok();
}

// A running await_changes stops at the client's notifications/cancelled (and the
// cancelled call gets no response) or at the end of input, even with
// timeout_seconds 0. Mirrors docs/frontends/mcp.md#the-work-loop.
#[cfg(unix)]
#[test]
fn await_changes_stops_on_cancellation_and_end_of_input() {
    use std::io::Write as _;
    let dir = bare_dir("await-cancel");
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
    let run = |script: Box<dyn FnOnce(std::os::unix::net::UnixStream) + Send>| -> (Vec<Value>, std::time::Duration) {
            let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
            let started = std::time::Instant::now();
            let feeder = std::thread::spawn(move || script(writer));
            let mut out: Vec<u8> = Vec::new();
            serving.serve(std::io::BufReader::new(reader), &mut out);
            feeder.join().unwrap();
            let replies = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
            (replies, started.elapsed())
        };
    let call = |id: u64, timeout: u64| {
        format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"tools/call\",\"params\":{{\"name\":\"await_changes\",\"arguments\":{{\"timeout_seconds\":{timeout}}}}}}}\n")
    };
    let (replies, took) = run(Box::new(move |mut w| {
        w.write_all(call(7, 60).as_bytes()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        w.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{\"requestId\":7}}\n").unwrap();
        w.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"ping\"}\n")
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5000));
    }));
    assert_eq!(
        replies,
        vec![json!({"jsonrpc":"2.0","id":8,"result":{}})],
        "the cancelled call answered"
    );
    assert!(took < std::time::Duration::from_secs(20), "{took:?}");
    let (replies, took) = run(Box::new(move |mut w| {
        w.write_all(call(9, 0).as_bytes()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
    }));
    assert!(
        took < std::time::Duration::from_secs(10),
        "end of input left the poll waiting: {took:?}"
    );
    assert!(replies.len() <= 1, "{replies:?}");
    crate::vfs::remove_dir_all(dir).ok();
}
