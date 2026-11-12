use super::*;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

mod chat;
mod recovery;
mod tasks;
mod workspace;

// An in-memory host record store; `.1` logs every read as `<stream>/<key>`.
#[derive(Default)]
pub(crate) struct MemoryRecords(
    pub(crate) std::sync::Mutex<BTreeMap<(String, String), (Value, Vec<(u64, Value)>)>>,
    pub(crate) std::sync::Mutex<Vec<String>>,
);

// The host every test on this thread talks to: one record store that outlives the
// workspaces a test reopens from snapshots, as a host's database does.
thread_local! {
    static HOST_RECORDS: Arc<MemoryRecords> = Arc::new(MemoryRecords::default());
}

pub(crate) fn host_records() -> Arc<MemoryRecords> {
    HOST_RECORDS.with(Arc::clone)
}

// `execution` with this thread's host record store, unless it names a store (or
// explicitly none, through `without_records`) of its own.
pub(crate) fn with_host(execution: &Execution) -> Execution {
    let mut execution = execution.clone();
    if execution.records.is_none() && !NO_RECORDS.with(std::cell::Cell::get) {
        execution.records = Some(host_records());
    }
    execution
}

thread_local! {
    static NO_RECORDS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

// While held, test requests on this thread run as a host without a record store.
pub(crate) struct WithoutRecords(bool);
pub(crate) fn without_records() -> WithoutRecords {
    WithoutRecords(NO_RECORDS.with(|n| n.replace(true)))
}
impl Drop for WithoutRecords {
    fn drop(&mut self) {
        NO_RECORDS.with(|n| n.set(self.0));
    }
}
impl crate::hosted::records::RecordStore for MemoryRecords {
    fn keys(&self, s: crate::hosted::records::Stream) -> Result<Vec<(String, Value)>, String> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|((st, _), _)| st == s.name())
            .map(|((_, k), (m, _))| (k.clone(), m.clone()))
            .collect())
    }
    fn meta(&self, s: crate::hosted::records::Stream, k: &str) -> Result<Option<Value>, String> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(&(s.name().into(), k.into()))
            .map(|(m, _)| m.clone()))
    }
    fn put_meta(
        &self,
        s: crate::hosted::records::Stream,
        k: &str,
        m: &Value,
    ) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .entry((s.name().into(), k.into()))
            .or_default()
            .0 = m.clone();
        Ok(())
    }
    fn append(
        &self,
        s: crate::hosted::records::Stream,
        k: &str,
        _g: u32,
        from: u64,
        items: &[Value],
    ) -> Result<(), String> {
        let mut m = self.0.lock().unwrap();
        let e = m.entry((s.name().into(), k.into())).or_default();
        for (i, v) in items.iter().enumerate() {
            let n = from + i as u64;
            if !e.1.iter().any(|(x, _)| *x == n) {
                e.1.push((n, v.clone()));
            }
        }
        Ok(())
    }
    fn replace(
        &self,
        s: crate::hosted::records::Stream,
        k: &str,
        _g: u32,
        items: &[Value],
    ) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .entry((s.name().into(), k.into()))
            .or_default()
            .1 = items
            .iter()
            .cloned()
            .enumerate()
            .map(|(n, v)| (n as u64, v))
            .collect();
        Ok(())
    }
    fn read(
        &self,
        s: crate::hosted::records::Stream,
        k: &str,
        after: Option<u64>,
        limit: usize,
    ) -> Result<Vec<(u64, Value)>, String> {
        self.1.lock().unwrap().push(format!("{}/{}", s.name(), k));
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(&(s.name().into(), k.into()))
            .map(|(_, v)| {
                v.iter()
                    .filter(|(n, _)| after.is_none_or(|a| *n > a))
                    .take(limit)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }
    fn delete(&self, s: crate::hosted::records::Stream, k: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(&(s.name().into(), k.into()));
        Ok(())
    }
}

// The conversation book as this thread's host record store holds it:
// `{seq, chats: {id: chat}}`, each chat its metadata with `updates` and `history`.
// Mirrors docs/compiler/hosted-runtime.md#host-records.
fn stored_book() -> Value {
    let records = host_records();
    let streams = records.0.lock().unwrap();
    let mut book = json!({"seq": 0, "chats": {}});
    for ((stream, key), (meta, items)) in streams.iter() {
        if stream != "chat-updates" {
            continue;
        }
        if key == "_book" {
            book["seq"] = meta["seq"].clone();
            continue;
        }
        let mut chat = meta.clone();
        if let Some(m) = chat.as_object_mut() {
            m.remove("_updatesGen");
            m.remove("_historyGen");
        }
        chat["updates"] = json!(items.iter().map(|(n, v)| json!([n, v])).collect::<Vec<_>>());
        chat["history"] = json!([]);
        book["chats"][key] = chat;
    }
    for ((stream, key), (_, items)) in streams.iter() {
        if stream == "chat-history" && book["chats"].get(key).is_some() {
            book["chats"][key]["history"] =
                Value::Array(items.iter().map(|(_, v)| v.clone()).collect());
        }
    }
    book
}

// A fresh project on a fresh host: this thread's host record store starts empty.
fn workspace(name: &str) -> Workspace {
    host_records().0.lock().unwrap().clear();
    Workspace::new(BTreeMap::from([
        (
            ".jazyk/jazyk.toml".into(),
            b"version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = [\"docs/main.md\"]\nsource = []\n"
                .to_vec(),
        ),
        (
            "docs/main.md".into(),
            format!("# {name}\n\nNavigation index.\n").into_bytes(),
        ),
    ]))
    .unwrap()
}

impl Workspace {
    // Existing fixture providers stay synchronous; every production invocation
    // below still exercises the new owned asynchronous host interface.
    fn request_test(
        &mut self,
        method: &str,
        path: &str,
        body: &str,
        execution: &Execution,
    ) -> Result<Reply, String> {
        let mut execution = with_host(execution);
        if execution.transport_async.is_none() {
            execution.transport_async = execution.transport.clone().map(|source| {
                Arc::new(move |body: String| {
                    let source = source.clone();
                    Box::pin(async move { source(&body) })
                        as std::pin::Pin<
                            Box<dyn std::future::Future<Output = Result<String, String>> + Send>,
                        >
                }) as ModelAsyncTransport
            });
        }
        if execution.transport_async_stream.is_none() {
            execution.transport_async_stream = execution.transport_stream.clone().map(|source| {
                Arc::new(move |body: String, sink: ModelDeltaSink| {
                    let source = source.clone();
                    Box::pin(async move { source(&body, &mut |field, text| sink(field, text)) })
                        as std::pin::Pin<
                            Box<dyn std::future::Future<Output = Result<String, String>> + Send>,
                        >
                }) as ModelAsyncStreamTransport
            });
        }
        self.request(method, path, body, &execution)
    }
}

fn value(w: &mut Workspace, method: &str, path: &str, body: Value, e: &Execution) -> Value {
    let reply = w.request_test(method, path, &body.to_string(), e).unwrap();
    assert!(
        reply.status < 400,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    serde_json::from_slice(&reply.body).unwrap()
}
fn model_reply(content: &str) -> String {
    json!({"choices":[{"message":{"role":"assistant","content":content}}],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}).to_string()
}

fn navigation_model(
    name: &'static str,
    calls: Arc<AtomicUsize>,
    barrier: Option<Arc<std::sync::Barrier>>,
) -> Execution {
    Execution {
        model: name.into(),
        transport: Some(Arc::new(move |body| {
            let request: Value = serde_json::from_str(body).unwrap();
            assert_eq!(request["model"], name);
            let n = calls.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                if let Some(b) = &barrier {
                    b.wait();
                }
            }
            if request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["role"] == "tool")
            {
                return Ok(model_reply("Done."));
            }
            let section = format!("docs/main.md#/{}", name.to_lowercase());
            let goal = format!("g:reconcile-section:{section}");
            let actions = [
                (
                    "set_coverage",
                    json!({"section":section,"state":"non-normative","note":"This navigation index contains no product obligations."}),
                ),
                (
                    "mark_goal_done",
                    json!({"goal":goal,"justification":"The section is a navigation index with no normative sentences."}),
                ),
                (
                    "done",
                    json!({"summary":"Classified the navigation index."}),
                ),
            ];
            Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":actions.iter().enumerate().map(|(i,(name,args))|json!({"id":format!("call-{i}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()}}],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}).to_string())
        })),
        ..Default::default()
    }
}
