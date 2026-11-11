use super::*;

impl McpServer {
    // ---- the agent-run benchmark ----
    // Mirrors docs/benchmark/benchmark.md#agent-run-benchmarks.

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn benchmark_cases(&self) -> Value {
        let cases = crate::benchmark::parse_cases();
        let b = self.bench.lock().unwrap();
        let scored: std::collections::BTreeMap<String, &Value> = b
            .scored
            .iter()
            .map(|e| (e["name"].as_str().unwrap_or("").to_string(), e))
            .collect();
        let rows: Vec<Value> = cases
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let state = if b.open.as_ref().map(|o| o.idx) == Some(i) {
                    json!("open")
                } else if let Some(e) = scored.get(&c.name) {
                    json!(format!("scored {}", e["score"]))
                } else {
                    json!("pending")
                };
                json!(format!(
                    "{} ({}, {}): {}",
                    c.name,
                    c.tier,
                    c.task_type,
                    state.as_str().unwrap_or("")
                ))
            })
            .collect();
        let done = b.scored.len();
        json!({
            "cases": rows,
            "scored": done,
            "total": cases.len(),
            "next": SERVING.get(if done >= cases.len() {
                "benchmark-scored"
            } else {
                "benchmark-begin"
            }),
        })
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn benchmark_cases(&self) -> Value {
        json!({"error": "the browser backend does not serve the benchmark"})
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn begin_case(&self, params: &Value) -> Value {
        let cases = crate::benchmark::parse_cases();
        let mut b = self.bench.lock().unwrap();
        if let Some(o) = &b.open {
            return json!({"error": {"rule": "case-open", "message": format!(
                "case `{}` is already open; finish_case first", cases[o.idx].name)}});
        }
        let scored: std::collections::BTreeSet<String> = b
            .scored
            .iter()
            .filter_map(|e| e["name"].as_str().map(String::from))
            .collect();
        let want = params["arguments"]["case"].as_str();
        let Some((idx, case)) = cases.iter().enumerate().find(|(_, c)| match want {
            Some(w) => c.name == w,
            None => !scored.contains(&c.name),
        }) else {
            return json!({"error": {"rule": "no-pending-case", "message": "no pending case; benchmark_cases shows the run, benchmark_report closes it"}});
        };
        let tmp = std::env::temp_dir().join(format!(
            "jazyk-mcp-bench-{}-{}",
            crate::sys::process_id(),
            case.name
        ));
        crate::vfs::remove_dir_all(&tmp).ok();
        // A derived-goal case seeds through a real commit and takes its goal off the
        // board, as an endpoint run does (docs/benchmark/benchmark.md#agent-run-benchmarks).
        let mut derived: Option<crate::model::Goal> = None;
        let store = if case.derives_goal() {
            match crate::benchmark::seed_derived(case, &tmp) {
                Ok((s, g)) => {
                    derived = Some(g);
                    s
                }
                Err(e) => {
                    return json!({"error": {"rule": "fixture", "message": e}});
                }
            }
        } else {
            if let Err(error) = crate::benchmark::prepare_runtime_fixture(case, &tmp) {
                return json!({"error": {"rule": "fixture", "message": error}});
            }
            crate::benchmark::sandbox(case, &tmp)
        };
        let gs = match crate::benchmark::fixture_gen_settings(&tmp) {
            Ok(gs) => gs,
            Err(error) => return json!({"error":{"rule":"fixture","message":error}}),
        };
        crate::vfs::create_dir_all(&gs.deliverable).ok();
        let item = match &derived {
            Some(g) => WorkItem::from_goal(g),
            None => WorkItem {
                task: case.task_type.clone(),
                target: case.target.clone(),
                dirty_sections: crate::benchmark::case_dirty_sections(&store, case),
                stale_anchors: Vec::new(),
                proposals: Vec::new(),
            },
        };
        let mut reply = json!({
            "case": {"name": case.name, "tier": case.tier, "task": case.task_type, "target": case.target},
            "next": SERVING.get("benchmark-case"),
        });
        match case.task_type.as_str() {
            "verify-requirement" => {
                if let Err(e) = crate::benchmark::seed_verification(case, &store, &gs) {
                    return json!({"error": {"rule": "fixture", "message": e}});
                }
                let r = &store.graph.requirements[&case.target];
                reply["instructions"] = json!(SERVING.get("benchmark-verify"));
                reply["package"] = json!({
                    "statement": r.statement,
                    "quote": r.source.as_ref().map(|s| s.quote.clone()).unwrap_or_default(),
                    "files": case.deliverable.keys().map(|f| gs.deliverable.join(f).to_string_lossy().to_string()).collect::<Vec<_>>(),
                });
            }
            _ => {
                if case.task_type == "generate-entity" {
                    reply["deliverableDir"] = json!(gs.deliverable.to_string_lossy());
                    reply["note"] = json!(SERVING.get("benchmark-generate"));
                }
            }
        }
        // One code path builds the case session, so an agent and an endpoint face the
        // same prompt, scope, loaded set, and skills
        // (docs/benchmark/benchmark.md#agent-run-benchmarks).
        let crate::benchmark::CaseSession {
            mut session,
            prompt,
            ..
        } = crate::benchmark::case_session(store.clone(), &item, &case.lint, &gs, derived.as_ref());
        if case.task_type != "verify-requirement" {
            reply["instructions"] = json!(prompt);
            reply["package"] = json!(session.render_status_full());
        }
        session.gen = gs.clone();
        session.caller = self.caller("benchmark", &case.name);
        b.open = Some(OpenCase {
            idx,
            item,
            derived,
            store,
            session,
            calls: 0,
            tmp,
            gs,
        });
        reply
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn begin_case(&self, _params: &Value) -> Value {
        json!({"error": "the browser backend does not serve the benchmark"})
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn finish_case(&self, params: &Value) -> Value {
        let cases = crate::benchmark::parse_cases();
        let mut b = self.bench.lock().unwrap();
        let Some(mut o) = b.open.take() else {
            return json!({"error": {"rule": "no-open-case", "message": "no case is open; begin_case first"}});
        };
        let case = &cases[o.idx];
        // Turn cases face the same done gates a turn does; a rejection keeps the case
        // open, same contract as done on a batch.
        let mut staged_count = 0usize;
        // Every case but generation and verification commits through done
        // (docs/benchmark/benchmark.md#agent-run-benchmarks).
        if !matches!(
            case.task_type.as_str(),
            "generate-entity" | "verify-requirement"
        ) {
            let summary = params["arguments"]["summary"]
                .as_str()
                .unwrap_or("(finish)")
                .to_string();
            if let Err(e) = o.session.dispatch("done", &json!({"summary": summary})) {
                let v = e.to_value();
                b.open = Some(o);
                return v;
            }
            let staged = std::mem::take(&mut o.session.staged);
            staged_count = staged.len();
            if !staged.is_empty() {
                let commit = match &o.derived {
                    Some(_) => o.session.commit(o.calls, None),
                    None => o.item.commit(o.calls, None),
                };
                o.store.apply(staged, &commit);
            }
        }
        if case.task_type == "verify-requirement" {
            let verdict = params["arguments"]["verdict"].as_str().unwrap_or("");
            if verdict != "pass" && verdict != "fail" {
                b.open = Some(o);
                return json!({"error": {"rule": "bad-argument", "message": "finish_case on a verification case needs verdict: pass or fail"}});
            }
            let evidence = params["arguments"]["evidence"].as_str().unwrap_or("");
            if let Err(e) = crate::verify::mark(
                &o.store,
                &case.target,
                verdict,
                None,
                Some(evidence),
                None,
                &o.gs,
            ) {
                b.open = Some(o);
                return json!({"error": {"rule": "bad-argument", "message": e}});
            }
        }
        // Grade: the same deterministic checks an endpoint run faces, over the
        // session's real staged count. A goal the model marked failed fails the case
        // with the model's reason; the checks are skipped and count as failed, as on
        // an abort: an untouched fixture satisfying a check is not evidence.
        // Mirrors docs/benchmark/benchmark.md#runs.
        let goal_failed = o
            .session
            .failed_goals()
            .into_iter()
            .next()
            .map(|(_, reason)| format!("goal marked failed: {}", reason));
        let (mut passed, mut fail): (usize, Option<String>) = (0, goal_failed);
        if fail.is_none() {
            for (kind, arg) in &case.checks {
                let verdict = if crate::benchmark::WORKFLOW_CHECKS.contains(&kind.as_str()) {
                    crate::benchmark::eval_workflow_check(kind, arg, &o.store, &o.gs, &case.target)
                } else {
                    crate::benchmark::eval_check(kind, arg, &o.store, staged_count)
                };
                match verdict {
                    None => passed += 1,
                    Some(why) => {
                        if fail.is_none() {
                            fail = Some(format!("{}: {}", kind, why));
                        }
                    }
                }
            }
        }
        let score = if case.checks.is_empty() {
            0.0
        } else {
            passed as f64 / case.checks.len() as f64
        };
        let efficiency = (case.par_rounds as f64 / o.calls.max(1) as f64).min(1.0);
        crate::vfs::remove_dir_all(&o.tmp).ok();
        let entry = json!({
            "name": case.name,
            "tier": case.tier,
            "score": (score * 100.0).round() / 100.0,
            "checks": format!("{}/{}", passed, case.checks.len()),
            "calls": o.calls,
            "parRounds": case.par_rounds,
            "efficiency": (efficiency * 100.0).round() / 100.0,
            "fail": fail.clone().unwrap_or_default(),
        });
        b.scored.retain(|e| e["name"] != entry["name"]);
        b.scored.push(entry.clone());
        let remaining = cases.len().saturating_sub(b.scored.len());
        json!({
            "scored": entry,
            "next": if remaining > 0 {
                SERVING.fill("benchmark-pending", &[("count", &remaining.to_string())])
            } else {
                SERVING.get("benchmark-scored").to_string()
            },
        })
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn finish_case(&self, _params: &Value) -> Value {
        json!({"error": "the browser backend does not serve the benchmark"})
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn benchmark_report(&self, params: &Value) -> Value {
        let mut b = self.bench.lock().unwrap();
        if b.open.is_some() {
            return json!({"error": {"rule": "case-open", "message": "a case is open; finish_case first"}});
        }
        if b.scored.is_empty() {
            return json!({"error": {"rule": "nothing-scored", "message": "no cases scored yet; begin_case starts the run"}});
        }
        let model = params["arguments"]["model"]
            .as_str()
            .map(String::from)
            .or_else(|| {
                self.client
                    .lock()
                    .unwrap()
                    .clone()
                    .map(|c| format!("{} (agent)", c))
            })
            .unwrap_or_else(|| "unnamed-agent".into());
        let mut tier_sum: std::collections::BTreeMap<&str, (f64, usize)> = Default::default();
        let mut tier_ok: std::collections::BTreeMap<&str, bool> = Default::default();
        let (mut eff_sum, mut checks_p, mut checks_t) = (0.0f64, 0usize, 0usize);
        for e in &b.scored {
            let t = crate::benchmark::tier_key(e["tier"].as_str().unwrap_or(""));
            let sc = e["score"].as_f64().unwrap_or(0.0);
            let en = tier_sum.entry(t).or_insert((0.0, 0));
            en.0 += sc;
            en.1 += 1;
            if sc < 1.0 {
                tier_ok.insert(t, false);
            }
            eff_sum += e["efficiency"].as_f64().unwrap_or(0.0);
            let parts: Vec<usize> = e["checks"]
                .as_str()
                .unwrap_or("0/0")
                .split('/')
                .filter_map(|x| x.parse().ok())
                .collect();
            if parts.len() == 2 {
                checks_p += parts[0];
                checks_t += parts[1];
            }
        }
        // A tier never attempted is unmeasured, not capable: a partial run says what
        // it graded and nothing more.
        let ran = |t: &str| tier_sum.contains_key(t);
        let ok = |t: &str| ran(t) && *tier_ok.get(t).unwrap_or(&true);
        let ts = |t: &str| {
            tier_sum
                .get(t)
                .map(|(s, n)| {
                    if *n == 0 {
                        0.0
                    } else {
                        ((s / *n as f64) * 100.0).round() / 100.0
                    }
                })
                .unwrap_or(0.0)
        };
        let report = json!({
            "verdicts": {
                "compilation": if !ran("extraction") { "unmeasured" } else {
                    match (ok("extraction"), ok("review") && ran("review")) {
                        (true, true) => "review", (true, false) => "extraction", _ => "not-capable",
                    }
                },
                "generation": if !ran("generation") { "unmeasured" } else if ok("generation") { "capable" } else { "not-capable" },
                "verification": if !ran("verification") { "unmeasured" } else if ok("verification") { "capable" } else { "not-capable" },
            },
            "scores": {"extraction": ts("extraction"), "review": ts("review"), "generation": ts("generation"), "verification": ts("verification")},
            "checks": format!("{}/{}", checks_p, checks_t),
            "efficiency": ((eff_sum / b.scored.len() as f64) * 100.0).round() / 100.0,
            "tokens": Value::Null,
            "throughput": Value::Null,
            "cases": b.scored.iter().map(|e| (e["name"].as_str().unwrap_or("").to_string(), e.clone())).collect::<std::collections::BTreeMap<String, Value>>(),
        });
        crate::benchmark::append_history(&model, "agent", &[("agent".to_string(), report.clone())]);
        b.scored.clear();
        json!({
            "model": model,
            "codec": "agent",
            "report": report,
            "verdictsLegend": "compilation verdicts order not-capable < extraction < review; review is the highest (extraction plus review judgment). generation and verification are capable or not-capable. unmeasured means the tier was never attempted.",
            "recorded": "appended to ~/.jazyk/benchmarks/history.yaml",
        })
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn benchmark_report(&self, _params: &Value) -> Value {
        json!({"error": "the browser backend does not serve the benchmark"})
    }
}
