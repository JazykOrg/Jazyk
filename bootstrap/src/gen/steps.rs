use super::*;

// One entity's task: the model picks the product path (FILE protocol, parts when
// dense), the tests path, and the manifest with the run commands. Requirements the
// model declares untestable programmatically, or whose declared test fails validation
// (name missing from the artifact, empty command), become llm rows with a criteria
// file. Manifest validation is deterministic; nothing here chooses for the model.
// Snapshot the previous content of a deliverable file before this run rewrites or
// removes it: the diff baseline frontends show against. A file the run creates fresh
// has no baseline, so a stale snapshot from an earlier run is dropped. File ownership
// keeps a path with one entity, so a per-gen_one set is once per run per file.
// Mirrors docs/consumers/gen.md#incremental-regeneration.
pub(super) fn snapshot_baseline(
    out: &Path,
    gs: &GenSettings,
    rel: &str,
    seen: &mut std::collections::HashSet<String>,
) {
    if !seen.insert(rel.to_string()) {
        return;
    }
    let dst = out.join("deliverable-baseline").join(rel);
    match crate::vfs::read(gs.deliverable.join(rel)) {
        Ok(bytes) => {
            if let Some(p) = dst.parent() {
                crate::vfs::create_dir_all(p).ok();
            }
            crate::vfs::write(&dst, bytes).ok();
        }
        Err(_) => {
            crate::vfs::remove_file(&dst).ok();
        }
    }
}

pub(super) fn snapshot_pipeline_baseline(
    out: &Path,
    gs: &GenSettings,
    rel: &str,
    seen: &mut std::collections::HashSet<String>,
    key: &str,
) -> Result<(), String> {
    if pipeline::first_baseline(key, rel)? {
        snapshot_baseline(out, gs, rel, seen);
    } else {
        seen.insert(rel.into());
    }
    Ok(())
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub async fn gen_one(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
    gs: &GenSettings,
    id: &str,
    task: &serde_json::Value,
) -> Result<usize, String> {
    pipeline::run(store, runner, gs, id, task).await
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub(super) async fn gen_one_inner(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
    gs: &GenSettings,
    id: &str,
    task: &serde_json::Value,
    ledger_snapshot: &Ledger,
    baseline_key: &str,
) -> Result<usize, String> {
    let mut baselined: std::collections::HashSet<String> = Default::default();
    let instructions = task["instructions"].as_str().unwrap_or_default();
    // The deliverable's medium is decided before any task runs; this task states it
    // and never re-decides it
    // (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
    let medium: Option<Medium> = serde_json::from_value(task["medium"].clone()).ok();
    let medium_line = match &medium {
        Some(m) => m.line(),
        None => STEPS.get("medium-undecided").to_string(),
    };
    let run_commands = task["runCommands"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    // A standing build failure means the artifact does not exist: whoever
    // regenerates next has to fix what broke (docs/consumers/gen.md#the-build).
    let build_error = match &task["buildError"] {
        serde_json::Value::Null => String::new(),
        e => {
            let yours: Vec<&str> = e["yours"]
                .as_array()
                .map(|a| a.iter().filter_map(|f| f.as_str()).collect())
                .unwrap_or_default();
            let whose = if yours.is_empty() {
                STEPS.get("build-broken-elsewhere").to_string()
            } else {
                STEPS.fill("build-broken-yours", &[("files", &yours.join(", "))])
            };
            format!(
                "\n{}\n",
                STEPS.fill(
                    "build-broken",
                    &[
                        ("error", e["error"].as_str().unwrap_or_default()),
                        ("whose", &(whose + " "))
                    ],
                )
            )
        }
    };
    let header = STEPS.fill(
        "header",
        &[
            ("medium", &medium_line),
            ("build_error", &build_error),
            ("id", id),
            ("name", task["name"].as_str().unwrap_or_default()),
            ("context", task["context"].as_str().unwrap_or_default()),
            (
                "changed",
                &task["changed"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default(),
            ),
            (
                "group",
                &match shared_with(store, id) {
                    v if v.is_empty() => String::new(),
                    v => STEPS.fill("header-group", &[("group", &v.join(", "))]),
                },
            ),
            (
                "generated",
                &serde_json::to_string(&task["generatedFiles"]).unwrap_or_default(),
            ),
            (
                "commands",
                if run_commands.is_empty() {
                    STEPS.get("header-commands-none")
                } else {
                    &run_commands
                },
            ),
            (
                "build",
                &match &task["build"] {
                    serde_json::Value::Null => STEPS.get("header-build-none").to_string(),
                    b => b.to_string(),
                },
            ),
        ],
    ) + "\n";
    // File ownership: a path recorded for an entity outside this one's component
    // group is off limits. One corrective retry, then the task fails.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    let ledger = ledger_snapshot;
    let owner_of = |path: &str| -> Option<String> { file_owner(store, ledger, id, path) };
    let groups = task["requirementGroups"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let parts = groups.len();
    let req_line = |r: &serde_json::Value| {
        STEPS.fill(
            "requirement",
            &[
                ("id", r["id"].as_str().unwrap_or("")),
                ("test", r["testName"].as_str().unwrap_or("")),
                ("statement", r["statement"].as_str().unwrap_or("")),
                ("quote", r["quote"].as_str().unwrap_or("")),
            ],
        )
    };

    // Files a reply wrote beside the step's own file. Whether each belongs to this
    // entity or to the deliverable is the manifest's call, made below
    // (docs/consumers/gen.md#file-ownership-and-conventions).
    let mut extra_written: Vec<String> = Vec::new();
    // Product content; the model names the file.
    let mut code = String::new();
    let mut product_rel = String::new();
    for (k, group) in groups.iter().enumerate() {
        let req_lines: Vec<String> = group
            .as_array()
            .map(|a| a.iter().map(req_line).collect())
            .unwrap_or_default();
        let user = if k == 0 {
            STEPS.fill(
                "product-first",
                &[
                    ("header", &header),
                    ("directive", &medium_directive(&medium)),
                    ("parts", &parts.to_string()),
                    ("requirements", &req_lines.join("\n")),
                ],
            ) + "\n"
        } else {
            STEPS.fill(
                "product-more",
                &[
                    ("header", &header),
                    ("group", &(k + 1).to_string()),
                    ("parts", &parts.to_string()),
                    ("requirements", &req_lines.join("\n")),
                    ("path", &product_rel),
                    ("code", &crate::llm::truncate(&code, 20_000)),
                ],
            )
        };
        let reply = runner
            .ask(
                instructions,
                &user,
                &format!("gen {}", id),
                &format!("product {}/{}", k + 1, parts),
            )
            .await
            .map_err(|e| pipeline::step_error(format!("product part {}/{}", k + 1, parts), e))?;
        if k == 0 {
            // Shape gets one corrective round here too: the product step is where a
            // long prompt most often costs the FILE line
            // (docs/consumers/gen.md#file-ownership-and-conventions).
            let mut written = match parse_file_replies(&reply) {
                Ok(v) => v,
                Err(e) => {
                    let retry = STEPS.fill(
                        "product-format-retry",
                        &[("request", &user), ("error", &e.to_string())],
                    ) + "\n";
                    let again = runner
                        .ask(
                            instructions,
                            &retry,
                            &format!("gen {}", id),
                            "product format retry",
                        )
                        .await
                        .map_err(|e| pipeline::step_error("product format retry".into(), e))?;
                    parse_file_replies(&again)?
                }
            };
            // The first file is this entity's part. Anything else the reply wrote is
            // deliverable-wide (an entry point, a config), so it lands as a support
            // file instead of being folded into the product and breaking it.
            let extra: Vec<(String, String)> = written.split_off(1);
            let (mut path, mut body) = written.remove(0);
            for (p, content) in extra {
                if owner_of(&p).is_some() {
                    continue;
                }
                let full = gs.deliverable.join(&p);
                if let Some(parent) = full.parent() {
                    crate::vfs::create_dir_all(parent).ok();
                }
                snapshot_pipeline_baseline(&store.out, gs, &p, &mut baselined, baseline_key)?;
                if crate::vfs::write(&full, format!("{}\n", content.trim_end())).is_ok() {
                    extra_written.push(p);
                }
            }
            if let Some(owner) = owner_of(&path) {
                let retry = STEPS.fill(
                    "owner-retry",
                    &[("request", &user), ("path", &path), ("owner", &owner)],
                );
                let reply2 = runner
                    .ask(
                        instructions,
                        &retry,
                        &format!("gen {}", id),
                        "product retry",
                    )
                    .await
                    .map_err(|e| pipeline::step_error("product retry".into(), e))?;
                let (p, b) = parse_file_reply(&reply2)?;
                if let Some(o) = owner_of(&p) {
                    return Err(format!("product path `{}` belongs to entity `{}`", p, o));
                }
                path = p;
                body = b;
            }
            product_rel = path;
            code = body;
        } else {
            code.push('\n');
            code.push_str(&strip_fences(&reply));
        }
        code.push('\n');
    }
    if code.trim().is_empty() {
        return Err("empty product".into());
    }
    let product_path = gs.deliverable.join(&product_rel);
    if let Some(p) = product_path.parent() {
        crate::vfs::create_dir_all(p).ok();
    }
    snapshot_pipeline_baseline(&store.out, gs, &product_rel, &mut baselined, baseline_key)?;
    crate::vfs::write(&product_path, &code).map_err(|e| e.to_string())?;

    // Tests: the model names the file too, or declares nothing programmatic.
    let all_reqs: Vec<serde_json::Value> = groups
        .iter()
        .flat_map(|g| g.as_array().cloned().unwrap_or_default())
        .collect();
    let req_lines: Vec<String> = all_reqs.iter().map(req_line).collect();
    let tests_user = STEPS.fill(
        "tests",
        &[
            ("header", &header),
            ("product", &product_rel),
            (
                "artifact_rule",
                &match &medium {
                    Some(m) if m.is_built() => {
                        STEPS.fill("tests-built", &[("artifact", &m.artifact)])
                    }
                    _ => STEPS.get("tests-default").to_string(),
                },
            ),
            ("requirements", &req_lines.join("\n")),
            ("code", &crate::llm::truncate(&code, 16_000)),
        ],
    ) + "\n";
    let tests_reply = runner
        .ask(instructions, &tests_user, &format!("gen {}", id), "tests")
        .await
        .map_err(|e| pipeline::step_error("tests file".into(), e))?;
    let mut files = vec![product_rel.clone()];
    // Files the manifest declares as the deliverable's, filled in once it is parsed
    // (docs/consumers/gen.md#file-ownership-and-conventions).
    let mut support_files: Vec<String> = Vec::new();
    let mut tests_rel = String::new();
    let mut tests_code = String::new();
    if tests_reply.trim() != "NONE" {
        // The shape is the harness's contract, and a weak model drops it under a long
        // prompt before it gets the content wrong: one corrective round, then fail.
        let parsed = match parse_file_replies(&tests_reply) {
            Ok(mut v) => {
                // Same rule as the product step: the first file is the tests
                // artifact, anything else the reply wrote is deliverable-wide.
                let extra: Vec<(String, String)> = v.split_off(1);
                for (p, content) in extra {
                    if owner_of(&p).is_some() {
                        continue;
                    }
                    let full = gs.deliverable.join(&p);
                    if let Some(parent) = full.parent() {
                        crate::vfs::create_dir_all(parent).ok();
                    }
                    snapshot_pipeline_baseline(&store.out, gs, &p, &mut baselined, baseline_key)?;
                    if crate::vfs::write(&full, format!("{}\n", content.trim_end())).is_ok() {
                        extra_written.push(p);
                    }
                }
                Some(v.remove(0))
            }
            Err(e) => {
                let retry = STEPS.fill(
                    "tests-format-retry",
                    &[("request", &tests_user), ("error", &e.to_string())],
                );
                let again = runner
                    .ask(
                        instructions,
                        &retry,
                        &format!("gen {}", id),
                        "tests format retry",
                    )
                    .await
                    .map_err(|e| pipeline::step_error("tests format retry".into(), e))?;
                if again.trim() == "NONE" {
                    None
                } else {
                    Some(parse_file_reply(&again).map_err(|e| format!("tests reply: {}", e))?)
                }
            }
        };
        if let Some((mut path, mut body)) = parsed {
            if let Some(owner) = owner_of(&path) {
                let retry = STEPS.fill(
                    "owner-retry",
                    &[("request", &tests_user), ("path", &path), ("owner", &owner)],
                );
                let reply2 = runner
                    .ask(instructions, &retry, &format!("gen {}", id), "tests retry")
                    .await
                    .map_err(|e| pipeline::step_error("tests retry".into(), e))?;
                let (p, b) =
                    parse_file_reply(&reply2).map_err(|e| format!("tests reply: {}", e))?;
                if let Some(o) = owner_of(&p) {
                    return Err(format!("tests path `{}` belongs to entity `{}`", p, o));
                }
                path = p;
                body = b;
            }
            tests_rel = path;
            tests_code = body;
            let tests_path = gs.deliverable.join(&tests_rel);
            if let Some(p) = tests_path.parent() {
                crate::vfs::create_dir_all(p).ok();
            }
            snapshot_pipeline_baseline(&store.out, gs, &tests_rel, &mut baselined, baseline_key)?;
            crate::vfs::write(&tests_path, &tests_code).map_err(|e| e.to_string())?;
            files.push(tests_rel.clone());
        }
    }

    // The manifest: the model declares run commands and any support files it needs;
    // support files are returned inline and written here.
    // Deterministic ground truth for the manifest step: the test names actually
    // present in the artifact. Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    let found_names: Vec<&str> = all_reqs
        .iter()
        .filter_map(|r| r["testName"].as_str())
        .filter(|n| !n.is_empty() && tests_code.contains(*n))
        .collect();
    // A run command that names neither the tests artifact nor the test runs the
    // product, not the test.
    let tests_base = std::path::Path::new(&tests_rel)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let runs_the_test = |run: &str, name: &str| -> bool {
        (!tests_rel.is_empty() && run.contains(&tests_rel))
            || (!tests_base.is_empty() && run.contains(&tests_base))
            || (!name.is_empty() && run.contains(name))
    };
    let manifest_user = STEPS.fill(
        "manifest",
        &[
            ("id", id),
            ("files", &format!("{:?}", files)),
            (
                "deliverable",
                task["deliverable"].as_str().unwrap_or_default(),
            ),
            (
                "commands",
                if run_commands.is_empty() {
                    STEPS.get("manifest-commands-none")
                } else {
                    &run_commands
                },
            ),
            (
                "build",
                &match &task["build"] {
                    serde_json::Value::Null => STEPS.get("manifest-build-none").to_string(),
                    b => b.to_string(),
                },
            ),
            (
                "entry",
                &match &task["buildEntry"] {
                    serde_json::Value::Null => STEPS.get("manifest-entry-none").to_string(),
                    e => STEPS.fill(
                        "manifest-entry",
                        &[
                            ("path", e["path"].as_str().unwrap_or_default()),
                            ("content", e["content"].as_str().unwrap_or_default()),
                        ],
                    ),
                },
            ),
            (
                "names",
                &if found_names.is_empty() {
                    STEPS.get("manifest-names-none").to_string()
                } else {
                    found_names.join(", ")
                },
            ),
            // The medium is already decided, so the build is an obligation or a
            // non-question; it is never the worker's call
            // (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
            (
                "build_rule",
                &match (&medium, &task["build"]) {
                    (Some(m), serde_json::Value::Null) if m.is_built() => STEPS.fill(
                        "manifest-build-required",
                        &[("form", &m.form), ("artifact", &m.artifact)],
                    ),
                    (_, serde_json::Value::Null) => STEPS.get("manifest-build-omit").to_string(),
                    (Some(m), _) if m.is_built() => {
                        STEPS.fill("manifest-build-reuse-built", &[("artifact", &m.artifact)])
                    }
                    _ => STEPS.get("manifest-build-reuse").to_string(),
                },
            ),
            (
                "requirements",
                &all_reqs
                    .iter()
                    .map(|r| {
                        STEPS.fill(
                            "manifest-requirement",
                            &[
                                ("id", r["id"].as_str().unwrap_or("")),
                                ("test", r["testName"].as_str().unwrap_or("")),
                            ],
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            ("tests", &tests_rel),
            ("tests_code", &crate::llm::truncate(&tests_code, 12_000)),
        ],
    ) + "\n";
    let manifest_reply = runner
        .ask(
            instructions,
            &manifest_user,
            &format!("gen {}", id),
            "manifest",
        )
        .await
        .map_err(|e| pipeline::step_error("manifest".into(), e))?;
    let parse_manifest = |reply: &str| -> Result<serde_json::Value, String> {
        let text = strip_fences(reply);
        let start = text.find('{').ok_or("the reply held no JSON object")?;
        let end = text.rfind('}').ok_or("the reply held no JSON object")?;
        serde_json::from_str(&text[start..=end]).map_err(|e| format!("{}", e))
    };
    let mut manifest_json: serde_json::Value = match parse_manifest(&manifest_reply) {
        Ok(v) => v,
        // Malformed JSON is a shape failure, not a content failure: one corrective
        // round with the parser's complaint quoted back.
        Err(e) => {
            let retry = STEPS.fill(
                "manifest-format-retry",
                &[("request", &manifest_user), ("error", &e.to_string())],
            );
            let again = runner
                .ask(
                    instructions,
                    &retry,
                    &format!("gen {}", id),
                    "manifest format retry",
                )
                .await
                .map_err(|e| pipeline::step_error("manifest format retry".into(), e))?;
            parse_manifest(&again).map_err(|e| format!("manifest JSON: {}", e))?
        }
    };
    // The manifest must agree with the artifact: a present test left undeclared, or a
    // declared programmatic test absent from the artifact, gets one corrective retry.
    // Rows still wrong after it fall back to llm in the validation below.
    let mut mismatches: Vec<String> = all_reqs
        .iter()
        .filter_map(|r| {
            let rid = r["id"].as_str().unwrap_or_default();
            let name = r["testName"].as_str().unwrap_or_default();
            let row = manifest_json["tests"].as_array().and_then(|a| {
                a.iter()
                    .find(|t| t["requirement"].as_str() == Some(rid))
                    .cloned()
            });
            // The suggested name is a suggestion: a test the generator named its own
            // way still counts, as long as the name it declared is in the artifact
            // (docs/consumers/gen.md#file-ownership-and-conventions).
            let declared_name = row
                .as_ref()
                .and_then(|t| t["name"].as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let present = (!name.is_empty() && tests_code.contains(name))
                || (!declared_name.is_empty() && tests_code.contains(&declared_name));
            let programmatic = row
                .as_ref()
                .map(|t| t["kind"].as_str() == Some("programmatic"))
                .unwrap_or(false);
            let run = row
                .as_ref()
                .and_then(|t| t["run"].as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let selector = if !declared_name.is_empty() && tests_code.contains(&declared_name) {
                &declared_name
            } else {
                name
            };
            let has_run = !run.is_empty() && runs_the_test(&run, selector);
            if present && !(programmatic && has_run) {
                Some(STEPS.fill(
                    "mismatch-undeclared",
                    &[("id", rid), ("test", name), ("tests", &tests_rel)],
                ))
            } else if !present && programmatic {
                Some(STEPS.fill("mismatch-absent", &[("id", rid), ("test", name)]))
            } else {
                None
            }
        })
        .collect();
    // The entry must actually compose the parts. The harness cannot read the medium,
    // but it can see whether the entry names the files the parts live in; an entry
    // that re-implements a part or drops one does not
    // (docs/consumers/gen.md#the-build).
    if let Some(m) = medium.as_ref().filter(|m| m.is_built()) {
        let entry_path = task["buildEntry"]["path"]
            .as_str()
            .map(String::from)
            .or_else(|| entry_from_run(manifest_json["build"]["run"].as_str().unwrap_or("")));
        // What the entry would hold after this task: the version it returns, or the
        // one already on disk when it returns none.
        let entry_content = manifest_json["supportFiles"]
            .as_array()
            .and_then(|a| {
                a.iter().find(|f| {
                    Some(norm_rel(f["path"].as_str().unwrap_or_default()))
                        == entry_path.as_ref().map(|p| norm_rel(p))
                })
            })
            .and_then(|f| f["content"].as_str())
            .map(String::from)
            // The package's copy was read before this task wrote anything, so the
            // file on disk is the fresher answer when the manifest returns none.
            .or_else(|| {
                entry_path
                    .as_ref()
                    .and_then(|p| crate::vfs::read_to_string(gs.deliverable.join(p)).ok())
                    .or_else(|| task["buildEntry"]["content"].as_str().map(String::from))
            });
        if let (Some(entry), Some(content)) = (&entry_path, &entry_content) {
            // Every part: this task's product file plus the ones other tasks wrote.
            let mut parts: Vec<String> = vec![product_rel.clone()];
            parts.extend(
                task["generatedFiles"]
                    .as_object()
                    .map(|o| {
                        o.values()
                            .flat_map(|v| v["files"].as_array().cloned().unwrap_or_default())
                            .filter_map(|f| f.as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            );
            let missing: Vec<String> = parts
                .into_iter()
                .map(|p| norm_rel(&p))
                .filter(|p| p != &norm_rel(entry) && !p.contains("test"))
                .filter(|p| {
                    let stem = std::path::Path::new(p)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string());
                    !stem.map(|st| content.contains(&st)).unwrap_or(true)
                })
                .collect();
            if !missing.is_empty() {
                mismatches.push(STEPS.fill(
                    "mismatch-entry",
                    &[
                        ("entry", entry),
                        ("artifact", &m.artifact),
                        ("missing", &missing.join(", ")),
                    ],
                ));
            }
        }
    }

    // A built medium with no build recorded has nothing to verify, so the missing
    // command joins the same corrective retry the artifact mismatches use
    // (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
    if let Some(m) = medium.as_ref().filter(|m| m.is_built()) {
        let recorded = !task["build"].is_null();
        let given = !manifest_json["build"]["run"]
            .as_str()
            .unwrap_or("")
            .trim()
            .is_empty();
        if !recorded && !given {
            mismatches.push(STEPS.fill(
                "mismatch-build",
                &[("form", &m.form), ("artifact", &m.artifact)],
            ));
        }
    }
    if !mismatches.is_empty() {
        // The previous answer goes back with the complaint: a correction changes what
        // the complaint names and nothing else
        // (docs/consumers/gen.md#file-ownership-and-conventions).
        let retry = STEPS.fill(
            "manifest-retry",
            &[
                ("request", &manifest_user),
                (
                    "answer",
                    &serde_json::to_string_pretty(&manifest_json).unwrap_or_default(),
                ),
                ("problems", &mismatches.join("\n")),
            ],
        );
        let retry_reply = runner
            .ask(
                instructions,
                &retry,
                &format!("gen {}", id),
                "manifest retry",
            )
            .await;
        if let Err(reason) = &retry_reply {
            if crate::session_recovery::is_interruption(reason) {
                return Err(reason.clone());
            }
        }
        if let Ok(reply2) = retry_reply {
            if let Ok(mut v) = parse_manifest(&reply2) {
                // Merge over the first answer: a retry that forgot a field keeps it.
                for key in ["supportFiles", "build", "choices"] {
                    let dropped = match &v[key] {
                        serde_json::Value::Null => true,
                        serde_json::Value::Array(a) => a.is_empty(),
                        serde_json::Value::Object(o) => o.is_empty(),
                        _ => false,
                    };
                    if dropped && !manifest_json[key].is_null() {
                        v[key] = manifest_json[key].clone();
                    }
                }
                manifest_json = v;
            }
        }
    }
    if let Some(support) = manifest_json["supportFiles"].as_array() {
        for f in support {
            let (Some(path), Some(content)) = (f["path"].as_str(), f["content"].as_str()) else {
                continue;
            };
            if path.starts_with('/') || path.contains("..") {
                continue;
            }
            // A support file belongs to the deliverable, which is exactly why it must
            // not land on a file some entity owns: this task's own product and tests
            // included. Ownership is what stops one file from eating another
            // (docs/consumers/gen.md#file-ownership-and-conventions).
            let owned = ledger
                .entities
                .values()
                .any(|e| e.files.iter().any(|f| f == path));
            if owned || path == product_rel || path == tests_rel {
                continue;
            }
            let p = gs.deliverable.join(path);
            if let Some(parent) = p.parent() {
                crate::vfs::create_dir_all(parent).ok();
            }
            snapshot_pipeline_baseline(&store.out, gs, path, &mut baselined, baseline_key)?;
            crate::vfs::write(&p, content).map_err(|e| e.to_string())?;
            support_files.push(path.to_string());
        }
    }

    // Deterministic validation: a programmatic row needs its declared test present in
    // the tests artifact and a non-empty command; anything else becomes an llm row.
    let declared = manifest_json["tests"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut tests_manifest: Vec<serde_json::Value> = Vec::new();
    for r in &all_reqs {
        let rid = r["id"].as_str().unwrap_or_default();
        let name = r["testName"].as_str().unwrap_or_default();
        let row = declared
            .iter()
            .find(|t| t["requirement"].as_str() == Some(rid));
        // The name the row is selected by: the suggestion when the artifact carries
        // it, otherwise whatever the generator named its test, as long as the artifact
        // carries that (docs/consumers/gen.md#file-ownership-and-conventions).
        let declared_name = row.and_then(|t| t["name"].as_str()).unwrap_or("").trim();
        let selector = if !name.is_empty() && tests_code.contains(name) {
            name
        } else if !declared_name.is_empty() && tests_code.contains(declared_name) {
            declared_name
        } else {
            ""
        };
        let programmatic = row
            .map(|t| {
                let run = t["run"].as_str().unwrap_or("").trim();
                t["kind"].as_str() == Some("programmatic")
                    && !run.is_empty()
                    && !selector.is_empty()
                    && runs_the_test(run, selector)
            })
            .unwrap_or(false);
        if programmatic {
            let t = row.unwrap();
            tests_manifest.push(serde_json::json!({
                "requirement": rid, "kind": "programmatic",
                "label": t["label"].as_str().unwrap_or("test"),
                "artifact": tests_rel, "name": selector,
                "run": t["run"].as_str().unwrap_or(""),
                "cwd": t["cwd"].as_str().unwrap_or("."),
                "files": [product_rel],
            }));
        } else {
            let crit_rel = r["criteriaPath"].as_str().unwrap_or_default().to_string();
            let crit_path = store.out.join("gen").join(&crit_rel);
            if let Some(p) = crit_path.parent() {
                crate::vfs::create_dir_all(p).ok();
            }
            let criteria = STEPS.fill(
                "llm-criteria",
                &[
                    ("id", rid),
                    ("hash", r["hash"].as_str().unwrap_or_default()),
                    ("statement", r["statement"].as_str().unwrap_or_default()),
                    ("quote", r["quote"].as_str().unwrap_or_default()),
                    ("files", &product_rel),
                ],
            ) + "\n";
            crate::vfs::write(&crit_path, criteria).ok();
            tests_manifest.push(serde_json::json!({
                "requirement": rid, "kind": "llm",
                "label": row.and_then(|t| t["label"].as_str()).unwrap_or("llm"),
                "artifact": crit_rel, "name": name,
                "run": format!("jazyk test {}", rid),
                "files": [product_rel],
            }));
        }
    }
    // The extra files a step wrote are sorted now that the manifest is known: the
    // deliverable keeps what the manifest calls support or runs as the build entry,
    // the entity keeps the rest (docs/consumers/gen.md#file-ownership-and-conventions).
    let declared_support: Vec<String> = manifest_json["supportFiles"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| f["path"].as_str().map(norm_rel))
                .collect()
        })
        .unwrap_or_default();
    let build_entry_path = task["buildEntry"]["path"]
        .as_str()
        .map(norm_rel)
        .or_else(|| {
            entry_from_run(manifest_json["build"]["run"].as_str().unwrap_or(""))
                .map(|p| norm_rel(&p))
        });
    for p in extra_written {
        let n = norm_rel(&p);
        let deliverable_wide =
            declared_support.contains(&n) || build_entry_path.as_deref() == Some(n.as_str());
        if deliverable_wide {
            support_files.push(p);
        } else if !files.contains(&p) {
            files.push(p);
        }
    }
    let mut manifest =
        serde_json::json!({"files": files, "tests": tests_manifest, "supportFiles": support_files});
    // The build the manifest step returned rides along: it is the deliverable's, not
    // this row's, and mark records it once (docs/consumers/gen.md#the-build).
    if !manifest_json["build"]["run"]
        .as_str()
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        manifest["build"] = manifest_json["build"].clone();
    }
    // The invented choices ride the manifest; mark validates them and the tool layer
    // stages their diagnostics (docs/consumers/gen.md#invented-choices).
    if manifest_json["choices"].is_array() {
        manifest["choices"] = manifest_json["choices"].clone();
    }
    // The record removes what the previous generation listed and this manifest omits
    // (docs/consumers/gen.md#incremental-regeneration).
    crate::gen::mark(store, id, task["factHash"].as_str(), &manifest, gs)?;
    // Both built-in workers file and clear the graded invented-choice diagnostics:
    // the session path stages them through record_generation, and this pipeline path
    // commits them here, right after the record they grade.
    // Mirrors docs/consumers/gen.md#invented-choices.
    let choices = parse_choices(store, &manifest)?;
    let unattached = Ledger::load(&store.out)
        .entities
        .get(crate::ids::entity_slug(id))
        .and_then(|e| e.unattached.clone());
    let mut ws = Store::load(&store.out);
    let ops = choice_ops(&ws, id, &choices, unattached.as_ref());
    if !ops.is_empty() {
        ws.apply(
            ops,
            &crate::store::Commit::session(vec![format!("g:generate:{}", id)], 0, None),
        );
    }
    Ok(files.len())
}
