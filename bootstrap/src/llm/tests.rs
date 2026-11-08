use super::*;
use std::sync::{Arc, Mutex};

#[test]
fn maintained_stream_decoder_preserves_utf8_reasoning_tools_usage_and_live_deltas() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;
    let chunks = [
        json!({"id":"reply-1","model":"fixture","choices":[{"delta":{"reasoning":"Check the rule. "}}]}),
        json!({"choices":[{"delta":{"content":"Hello 世界"}}]}),
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"search","arguments":"{\"query\":"}}]}}]}),
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"Customer\"}"}}]},"finish_reason":"tool_calls"}]}),
        json!({"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":7,"total_tokens":107,"prompt_tokens_details":{"cached_tokens":64}}}),
    ];
    let wire = chunks.iter().map(|chunk| format!("data: {chunk}\r\n\r\n")).collect::<String>() + "data: [DONE]\r\n\r\n";
    let fragments = wire.as_bytes().chunks(13).map(|bytes| Ok::<_, std::io::Error>(bytes.to_vec())).collect::<Vec<_>>();
    let events = futures::stream::iter(fragments).eventsource().map(|event| event
        .map(|event| format!("data: {}", event.data)).map_err(anyhow::Error::from));
    let seen = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let capture = seen.clone();
    let sink: crate::harness::ModelDeltaSink = Arc::new(move |field, text| capture.lock().unwrap().push((field.into(), text.into())));
    let body = async_io::block_on(collect_native_stream(Box::pin(events), Some(sink))).unwrap();
    let body: Value = serde_json::from_str(&body).unwrap();
    let message: goose_provider_types::conversation::message::Message = serde_json::from_value(body["_jazykGdkResponse"]["message"].clone()).unwrap();
    assert_eq!(message.as_concat_text(), "Hello 世界");
    let request = message.content.iter().find_map(goose_provider_types::conversation::message::MessageContent::as_tool_request).unwrap();
    assert_eq!(request.tool_call.as_ref().unwrap().arguments.as_ref().unwrap().get("query"), Some(&json!("Customer")));
    let usage = &body["_jazykGdkResponse"]["usage"]["usage"];
    assert_eq!(usage["total_tokens"], 107);
    assert_eq!(usage["cache_read_input_tokens"], 64);
    let deltas = seen.lock().unwrap();
    assert_eq!(deltas.iter().filter(|(field,_)| field == "content").map(|(_,text)| text.as_str()).collect::<String>(), "Hello 世界");
    assert_eq!(deltas.iter().filter(|(field,_)| field == "reasoning_content").map(|(_,text)| text.as_str()).collect::<String>(), "Check the rule. ");
}

#[test]
fn stop_releases_a_native_gdk_request_before_the_provider_replies() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (release, wait) = std::sync::mpsc::channel();
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = cancel.clone();
    let provider = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 8192];
        stream.read(&mut request).unwrap();
        flag.store(true, Ordering::SeqCst);
        wait.recv().unwrap();
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"late"}}]}"#;
        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
    });
    let llm = Llm { api: "chat_completions".into(), reasoning_summary: None,  base_url:format!("http://{address}"), model:"fixture".into(), api_key:String::new(), temperature:None, trace:None };
    let mut history = vec![json!({"role":"user","content":"Run"})];
    let started = std::time::Instant::now();
    let stop = crate::harness::run_loop(crate::harness::LoopArgs {
        llm:&llm, history:&mut history, tools:&[], dispatch:&mut |_, _| panic!(), emit:&mut |_| {},
        cancelled:&|| cancel.load(Ordering::SeqCst), max_rounds:1, label:"native stop".into(), conversation:true, inputs:None, input_close:None,
    });
    assert_eq!(stop, crate::harness::Stop::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(2));
    release.send(()).unwrap();
    provider.join().unwrap();
}

// A run's requests record only the messages past what the previous request repeated,
// so a reader rebuilds each request from the run's earlier ones.
// Mirrors docs/compiler/sessions.md#trace-events.
#[test]
fn a_request_records_only_what_changed_since_the_runs_previous_request() {
    assert_eq!(run_of("chat 7:3:Inference:0"), "chat 7");
    assert_eq!(run_of("b1:with:colons:12:Compaction:1"), "b1:with:colons");
    assert_eq!(run_of("one-shot"), "one-shot");
    let trace = crate::session::Trace::stderr(crate::session::TraceLevel::Quiet);
    let a = json!({"role":"system","content":"contract"});
    let b = json!({"role":"user","content":"task"});
    let c = json!({"role":"assistant","content":"done"});
    assert_eq!(trace.request_prefix("r", &[a.clone(), b.clone()]), 0);
    assert_eq!(trace.request_prefix("r", &[a.clone(), b.clone(), c.clone()]), 2);
    assert_eq!(trace.request_prefix("other", &[a.clone(), b.clone()]), 0, "runs are separate");
    assert_eq!(trace.request_prefix("r", &[a.clone(), c.clone()]), 1, "a rewritten history repeats only its common start");
    let clone = trace.clone();
    assert_eq!(clone.request_prefix("r", &[a, c]), 2, "clones share the record");
}
