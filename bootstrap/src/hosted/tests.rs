use super::*;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

mod chat;
mod recovery;
mod tasks;
mod workspace;

// An in-memory host record store; `.1` logs every read as `<stream>/<key>`.
#[derive(Default)]
pub(super) struct MemoryRecords(
    pub(super) std::sync::Mutex<BTreeMap<(String, String), (Value, Vec<(u64, Value)>)>>,
    pub(super) std::sync::Mutex<Vec<String>>,
);
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

// The conversation book as the file-backed record store holds it in a snapshot:
// `{seq, chats: {id: chat}}`, each chat its metadata with `updates` and `history`.
// Mirrors docs/compiler/hosted-runtime.md#host-records.
fn stored_book(files: &Snapshot) -> Value {
    let prefix = ".jazyk/local/records/";
    let docs = |stream: &str| -> Vec<Value> {
        files
            .iter()
            .filter(|(p, _)| p.starts_with(&format!("{prefix}{stream}/")) && p.ends_with(".json"))
            .filter_map(|(_, b)| serde_json::from_slice::<Value>(b).ok())
            .collect()
    };
    let mut book = json!({"seq": 0, "chats": {}});
    for doc in docs("chat-updates") {
        let key = doc["key"].as_str().unwrap_or_default().to_string();
        if key == "_book" {
            book["seq"] = doc["meta"]["seq"].clone();
            continue;
        }
        let mut chat = doc["meta"].clone();
        if let Some(m) = chat.as_object_mut() {
            m.remove("_updatesGen");
            m.remove("_historyGen");
        }
        chat["updates"] = doc["items"].clone();
        chat["history"] = json!([]);
        book["chats"][&key] = chat;
    }
    for doc in docs("chat-history") {
        let key = doc["key"].as_str().unwrap_or_default().to_string();
        if book["chats"].get(&key).is_some() {
            book["chats"][&key]["history"] = Value::Array(
                doc["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|i| i[1].clone())
                    .collect(),
            );
        }
    }
    book
}

fn workspace(name: &str) -> Workspace {
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
        let mut execution = execution.clone();
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
