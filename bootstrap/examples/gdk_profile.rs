//! Opt-in synthetic profile of the real GDK runtime, not a capacity benchmark.
//! Run with tests/gdk_profile.py to sample native RSS and thread counts.
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use jazyk::harness::*;
    use serde_json::{json, Value};
    use std::{
        cell::{Cell, RefCell},
        io::Write,
        rc::Rc,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::{Duration, Instant},
    };

    #[derive(Default)]
    struct Metrics {
        first_model_us: Vec<u64>,
        checkpoints_us: Vec<u64>,
        checkpoint_bytes: Vec<usize>,
        model_requests: usize,
        tools: usize,
    }

    struct Host {
        round: usize,
        delay: Duration,
        start: Instant,
        metrics: Rc<RefCell<Metrics>>,
    }
    impl ModelHost for Host {
        fn infer<'a>(
            &'a mut self,
            _: ModelRequest,
        ) -> LocalFuture<'a, Result<ModelResponse, String>> {
            Box::pin(async move {
                {
                    let mut metrics = self.metrics.borrow_mut();
                    metrics.model_requests += 1;
                    if self.round == 0 {
                        metrics
                            .first_model_us
                            .push(self.start.elapsed().as_micros() as u64);
                    }
                }
                async_io::Timer::after(self.delay).await;
                let message = if self.round < 2 {
                    json!({"role":"assistant","content":"","tool_calls":[{"id":"reused-provider-id","type":"function","function":{"name":"echo","arguments":"{\"value\":7}"}}]})
                } else {
                    json!({"role":"assistant","content":"Complete."})
                };
                self.round += 1;
                Ok(ModelResponse {
                    body: json!({"choices":[{"message":message,"finish_reason":if self.round<=2 {"tool_calls"} else {"stop"}}],
                    "usage":{"prompt_tokens":2100,"completion_tokens":20,"total_tokens":2120}}),
                })
            })
        }
    }

    struct Store {
        state: Option<HarnessState>,
        pause_after_tool: bool,
        pause: bool,
        metrics: Rc<RefCell<Metrics>>,
    }
    impl Recovery for Store {
        fn load<'a>(&'a mut self) -> LocalFuture<'a, Result<Option<HarnessState>, String>> {
            Box::pin(async { Ok(None) })
        }
        fn checkpoint<'a>(
            &'a mut self,
            boundary: CheckpointBoundary,
            state: &'a HarnessState,
        ) -> LocalFuture<'a, Result<(), String>> {
            Box::pin(async move {
                let start = Instant::now();
                let bytes = serde_json::to_vec(state).map_err(|error| error.to_string())?;
                let elapsed = start.elapsed().as_micros() as u64;
                let mut metrics = self.metrics.borrow_mut();
                metrics.checkpoints_us.push(elapsed);
                metrics.checkpoint_bytes.push(bytes.len());
                drop(metrics);
                self.state = Some(state.clone());
                if self.pause_after_tool && boundary.phase == CheckpointPhase::AfterTool {
                    self.pause = true;
                }
                Ok(())
            })
        }
        fn suspend_requested(&self) -> bool {
            self.pause
        }
    }

    struct Outcome {
        stop: Stop,
        finished_us: u64,
        _state: Option<HarnessState>,
    }

    async fn session(
        index: usize,
        delay: Duration,
        pause: bool,
        start: Instant,
        cancel: Arc<AtomicBool>,
        metrics: Rc<RefCell<Metrics>>,
    ) -> Outcome {
        let owner = std::thread::current().id();
        let llm = jazyk::llm::Llm { api: "chat_completions".into(), reasoning_summary: None,
            base_url: "http://127.0.0.1/unused".into(),
            model: "profile".into(),
            api_key: String::new(),
            temperature: None,
            trace: None,
        };
        let mut history =
            vec![json!({"role":"user","content":"A representative session context. ".repeat(256)})];
        let tools = vec![GenericTool {
            name: "echo".into(),
            description: "Echo a host-owned virtual tool result.".into(),
            parameters: json!({"type":"object","properties":{"value":{"type":"integer"}},"required":["value"]}),
        }];
        let mut host = Host {
            round: 0,
            delay,
            start,
            metrics: metrics.clone(),
        };
        let mut store = Store {
            state: None,
            pause_after_tool: pause,
            pause: false,
            metrics: metrics.clone(),
        };
        let mut dispatch = |name: &str, args: &Value| {
            assert_eq!(std::thread::current().id(), owner);
            assert_eq!(name, "echo");
            assert_eq!(args["value"], 7);
            metrics.borrow_mut().tools += 1;
            Ok("A virtual result. ".repeat(16))
        };
        let stop = run_with_host_async(
            LoopArgs {
                llm: &llm,
                history: &mut history,
                tools: &tools,
                dispatch: &mut dispatch,
                emit: &mut |_| {},
                cancelled: &|| cancel.load(Ordering::Acquire),
                max_rounds: 4,
                label: format!("profile-{index}"),
                conversation: false,
                inputs: None,
                input_close: None,
            },
            "Use the supplied host tool and return a short result.",
            &mut host,
            &mut store,
        )
        .await;
        Outcome {
            stop,
            finished_us: start.elapsed().as_micros() as u64,
            _state: store.state,
        }
    }

    fn percentile<T: Copy + Ord + Default>(values: &[T], percent: usize) -> T {
        let mut values = values.to_vec();
        values.sort();
        values
            .get(values.len().saturating_sub(1) * percent / 100)
            .copied()
            .unwrap_or_default()
    }
    fn output(value: Value) {
        println!("{value}");
        std::io::stdout().flush().unwrap();
    }
    fn argument(name: &str, default: usize) -> usize {
        let args: Vec<_> = std::env::args().collect();
        args.windows(2)
            .find(|pair| pair[0] == name)
            .and_then(|pair| pair[1].parse().ok())
            .unwrap_or(default)
    }
    pub fn main() {
        let active = argument("--active", 8);
        let idle = argument("--idle", 100);
        let delay = argument("--delay-ms", 100);
        let cancel_every = argument("--cancel-every", 0);
        async_io::block_on(async {
            let mut idle_sessions = Vec::new();
            let idle_metrics = Rc::new(RefCell::new(Metrics::default()));
            for index in 0..idle {
                let result = session(
                    index,
                    Duration::ZERO,
                    true,
                    Instant::now(),
                    Arc::new(AtomicBool::new(false)),
                    idle_metrics.clone(),
                )
                .await;
                assert_eq!(result.stop, Stop::Suspended);
                idle_sessions.push(result);
            }
            output(
                json!({"event":"ready","pid":std::process::id(),"active":active,"idle":idle,
                "idleCapsuleBytes":idle_sessions.iter().filter_map(|session|session._state.as_ref()).map(|state|serde_json::to_vec(state).unwrap().len()).sum::<usize>()}),
            );
            async_io::Timer::after(Duration::from_millis(200)).await;
            let start = Instant::now();
            let metrics = Rc::new(RefCell::new(Metrics::default()));
            let cancellations: Vec<_> = (0..active)
                .map(|_| Arc::new(AtomicBool::new(false)))
                .collect();
            let cancel_requested = Rc::new(Cell::new(None));
            let tasks = cancellations.iter().enumerate().map(|(index, cancel)| {
                session(
                    index,
                    Duration::from_millis(delay as u64),
                    false,
                    start,
                    cancel.clone(),
                    metrics.clone(),
                )
            });
            output(json!({"event":"running"}));
            let cancel = async {
                async_io::Timer::after(Duration::from_millis(120)).await;
                if cancel_every > 0 {
                    cancel_requested.set(Some(start.elapsed().as_micros() as u64));
                    for (index, cancel) in cancellations.iter().enumerate() {
                        if index % cancel_every == 0 {
                            cancel.store(true, Ordering::Release);
                        }
                    }
                }
            };
            let (outcomes, ()) = futures::join!(futures::future::join_all(tasks), cancel);
            let metrics = metrics.borrow();
            let latencies: Vec<_> = outcomes
                .iter()
                .filter(|outcome| outcome.stop == Stop::Cancelled)
                .map(|outcome| {
                    outcome
                        .finished_us
                        .saturating_sub(cancel_requested.get().unwrap())
                })
                .collect();
            let errors: Vec<_> = outcomes
                .iter()
                .filter_map(|outcome| match &outcome.stop {
                    Stop::Error(error) => Some(error),
                    _ => None,
                })
                .collect();
            assert!(errors.is_empty(), "{errors:?}");
            output(
                json!({"event":"result","active":active,"idle":idle,"modelDelayMs":delay,"cancelEvery":cancel_every,
                "elapsedUs":start.elapsed().as_micros() as u64,"completed":outcomes.iter().filter(|outcome|outcome.stop==Stop::EndTurn).count(),
                "cancelled":latencies.len(),"models":metrics.model_requests,"tools":metrics.tools,
                "firstModelUs":{"min":metrics.first_model_us.iter().min(),"p50":percentile(&metrics.first_model_us,50),"p95":percentile(&metrics.first_model_us,95),"max":metrics.first_model_us.iter().max()},
                "cancelLatencyUs":{"p50":percentile(&latencies,50),"max":latencies.iter().max()},
                "checkpoints":{"count":metrics.checkpoints_us.len(),"serializationUsP50":percentile(&metrics.checkpoints_us,50),"serializationUsP95":percentile(&metrics.checkpoints_us,95),
                    "maxBytes":metrics.checkpoint_bytes.iter().max(),"totalBytes":metrics.checkpoint_bytes.iter().sum::<usize>()}}),
            );
            // Keep owned session state resident long enough for external sampling.
            async_io::Timer::after(Duration::from_millis(200)).await;
            std::hint::black_box((idle_sessions, outcomes));
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    native::main();
}
#[cfg(target_arch = "wasm32")]
fn main() {}
