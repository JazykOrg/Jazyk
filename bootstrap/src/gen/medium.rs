use super::*;

// The tool family a word of a run command or a toolchain description names. Unknown
// words name none, so a divergence is only ever declared between two known families.
// Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
fn tool_family(word: &str) -> Option<&'static str> {
    let w = word
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '+' && c != '#')
        .to_lowercase();
    let w = w.rsplit('/').next().unwrap_or(&w);
    Some(match w {
        "cargo" | "rustc" | "rust" => "rust",
        "python" | "python3" | "pytest" | "pip" | "pip3" | "poetry" | "uv" => "python",
        "node" | "npm" | "npx" | "yarn" | "pnpm" | "jest" | "vitest" | "mocha" | "tsc" | "deno"
        | "bun" | "javascript" | "typescript" => "node",
        "go" | "golang" => "go",
        "mvn" | "maven" | "gradle" | "java" | "javac" | "kotlin" => "jvm",
        "dotnet" | "csharp" | "c#" => "dotnet",
        "ruby" | "rspec" | "bundle" | "gem" => "ruby",
        "swift" => "swift",
        "gcc" | "clang" | "g++" | "make" | "cmake" | "ctest" => "c",
        "php" | "phpunit" | "composer" => "php",
        _ => return None,
    })
}

// The decided medium's toolchain against the recorded programmatic run commands.
// Some when both name a tool family and share none: the medium diverged from what
// the sessions actually wrote. None when either side names no known family.
// Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
// The note a package carries while no session has decided the medium: an external
// worker decides and records it with record_medium before writing anything.
// Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
pub fn medium_note(ledger: &Ledger) -> Option<String> {
    if ledger.medium.is_some() {
        return None;
    }
    Some(
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/gen-medium/medium-undecided-no-session-has-decided-what-2a1a4.md"
        ))
        .to_string(),
    )
}

pub fn medium_divergence(ledger: &Ledger) -> Option<String> {
    let medium = ledger.medium.as_ref()?;
    let mut toolchain: Vec<&str> = medium
        .toolchain
        .split_whitespace()
        .filter_map(tool_family)
        .collect();
    toolchain.sort();
    toolchain.dedup();
    if toolchain.is_empty() {
        return None;
    }
    // One family per command: the first known tool word, so `sh -c 'cargo test'`
    // reads as Rust and `python -m pytest` as Python.
    let mut commands: Vec<(&'static str, &str)> = ledger
        .requirements
        .values()
        .filter(|r| r.test.kind == "programmatic")
        .filter_map(|r| {
            r.test
                .run
                .split_whitespace()
                .find_map(tool_family)
                .map(|f| (f, r.test.run.as_str()))
        })
        .collect();
    commands.sort();
    commands.dedup();
    if commands.is_empty() || commands.iter().any(|(f, _)| toolchain.contains(f)) {
        return None;
    }
    let mut families: Vec<&str> = commands.iter().map(|(f, _)| *f).collect();
    families.dedup();
    let sample: Vec<String> = commands
        .iter()
        .take(3)
        .map(|(_, run)| format!("`{}`", run))
        .collect();
    Some(format!(
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/gen-medium/the-decided-medium-s-toolchain-names-while-d492e.md"
        )),
        medium.toolchain,
        toolchain.join(", "),
        families.join(", "),
        sample.join(", ")
    ))
}

// What the deliverable is made of, decided once for the whole deliverable before any
// entity generates, then carried by every task package. A per-task decision is where
// prose gets substituted for the artifact.
// Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
#[derive(Serialize, Deserialize, Clone)]
pub struct Medium {
    pub form: String,
    // written: the generated files are the deliverable.
    // built: they are the source that produces `artifact`.
    pub produced: String,
    #[serde(default)]
    pub toolchain: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub artifact: String,
}

// The generation pipeline's prompts: the medium decision, the per-entity steps, their
// corrective retries, and the llm criteria file. Mirrors docs/compiler/sessions.md#payload-files.
pub(crate) const STEPS: crate::payload::Fragments =
    crate::payload::Fragments(include_str!(concat!(
        env!("OUT_DIR"),
        "/payloads/goals/prompts/generation/generation-steps.md.mustache"
    )));

impl Medium {
    pub fn is_built(&self) -> bool {
        self.produced == "built"
    }
    // The decision as the task packages and prompts state it: one line, no hedging.
    pub fn line(&self) -> String {
        if self.is_built() {
            STEPS.fill(
                "medium-line-built",
                &[
                    ("form", &self.form),
                    ("toolchain", &self.toolchain),
                    ("artifact", &self.artifact),
                ],
            )
        } else {
            STEPS.fill(
                "medium-line-written",
                &[("form", &self.form), ("toolchain", &self.toolchain)],
            )
        }
    }
}

// The one command that produces the deliverable's artifact, per deliverable, not per
// entity. Runs once before any row is judged.
#[derive(Serialize, Deserialize, Clone)]
pub struct Build {
    pub run: String,
    #[serde(default = "dot")]
    pub cwd: String,
    // Deliverable-relative paths the command creates. A missing one fails the run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub produces: Vec<String>,
    // What happened the last time the build ran. A failure is a fact the next
    // generation task must see, or it writes the same broken part again
    // (docs/consumers/gen.md#the-build). The alias reads a ledger written before the
    // key took its documented spelling.
    #[serde(
        default,
        rename = "lastRun",
        alias = "last_run",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_run: Option<BuildRun>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BuildRun {
    pub at: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
}

// Run the deliverable's build once and record what happened. Ok(()) when there is no
// build to run or it produced everything it promised; the error is what a reader
// needs to see. Mirrors docs/consumers/gen.md#the-build.
pub fn run_build(
    out: &Path,
    gs: &GenSettings,
    trace: &crate::session::Trace,
    label: &str,
) -> Result<(), String> {
    run_build_receipt(out, gs, trace, label).map(|_| ())
}

pub(crate) fn run_build_receipt(
    out: &Path,
    gs: &GenSettings,
    trace: &crate::session::Trace,
    label: &str,
) -> Result<Option<(String, String, Value)>, String> {
    let Some(b) = Ledger::load(out).build.clone() else {
        return Ok(None);
    };
    if let Some(id) = b.run.strip_prefix("task:") {
        let root = crate::task_runtime::project_root(out)?;
        let ledger = Ledger::load(out);
        let files: Vec<String> = ledger
            .entities
            .values()
            .flat_map(|row| row.files.iter().cloned())
            .chain(ledger.support.iter().cloned())
            .collect();
        let inputs = hash_hex(
            &json!({"run":b.run,"cwd":b.cwd,"produces":b.produces,
            "sources":hash_files(gs,&files)})
            .to_string(),
        );
        let (run_id, result) = crate::task_runtime::execute_with_key(
            &root,
            id,
            &format!("build:{label}:{}:{inputs}", out.display()),
        )?;
        let success = result["status"] == "succeeded" && result["complete"] == true;
        if !success {
            record_build_run(
                out,
                false,
                result["log"].as_str().unwrap_or("No log output"),
            );
            return Err("Build task did not complete successfully. Inspect its run log.".into());
        }
        check_build_products(out, gs, &b)?;
        return Ok(Some((id.to_owned(), run_id, result)));
    }
    let cwd = gs.deliverable.join(&b.cwd);
    trace.line(label, &format!("build: {} (in {})", b.run, cwd.display()));
    let done = crate::sys::run_shell(&b.run, &cwd).map_err(|e| {
        format!(
            "build `{}` could not start in {}: {}",
            b.run,
            cwd.display(),
            e
        )
    })?;
    if !done.success {
        let mut evidence = String::from_utf8_lossy(&done.stdout).to_string();
        evidence.push_str(&String::from_utf8_lossy(&done.stderr));
        // The next generation task needs to see this; printing it here only reaches
        // whoever is watching (docs/consumers/gen.md#the-build).
        record_build_run(out, false, &evidence);
        return Err(format!(
            "build `{}` failed ({}). Output:\n{}",
            b.run,
            done.status(),
            crate::llm::truncate(evidence.trim(), 2000)
        ));
    }
    check_build_products(out, gs, &b)?;
    Ok(None)
}

pub(crate) fn check_build_products(out: &Path, gs: &GenSettings, b: &Build) -> Result<(), String> {
    let missing: Vec<&String> = b
        .produces
        .iter()
        .filter(|p| !gs.deliverable.join(p).vexists())
        .collect();
    if !missing.is_empty() {
        let names = missing
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        record_build_run(
            out,
            false,
            &format!("exited 0 but did not produce {}", names),
        );
        return Err(format!(
            "build `{}` exited 0 but did not produce {}",
            b.run, names
        ));
    }
    record_build_run(out, true, "");
    Ok(())
}

// Record what the build did, for the next task package. Best effort: a ledger that
// cannot be written does not fail a verification run.
pub fn record_build_run(out: &Path, ok: bool, error: &str) {
    let _lock = Ledger::lock(out);
    let mut ledger = Ledger::load(out);
    if let Some(b) = ledger.build.as_mut() {
        b.last_run = Some(BuildRun {
            at: crate::verify::now_iso(),
            ok,
            error: crate::llm::truncate(error.trim(), 1500),
        });
        let _ = ledger.save(out);
    }
}

// The deliverable's current file listing, relative paths, two levels deep, capped:
// enough to pin a medium (a Cargo.toml, a src/ tree) without flooding the prompt.
pub(super) fn list_existing_files(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>, depth: usize) {
        if depth > 2 || out.len() >= 40 {
            return;
        }
        let Ok(rd) = crate::vfs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<crate::vfs::DirEntry> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if out.len() >= 40 {
                return;
            }
            let p = e.path();
            let Ok(relative) = p.strip_prefix(root) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            if !crate::document_sets::safe_relative(root, &relative) {
                continue;
            }
            if p.vis_dir() {
                walk(&p, root, out, depth + 1);
            } else {
                out.push(relative);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out, 0);
    out
}

// Decide what the deliverable is made of, once, from the statements that say so. The
// answer is recorded in the ledger and stated as a fact to every later task, so no
// per-entity task has to work it out again.
// Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub async fn decide_medium(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
    deliverable: &Path,
) -> Result<Medium, String> {
    // Every statement in the graph, capped: the medium is stated somewhere in the
    // documents, and which statement says it is exactly what the model must find.
    let mut statements: Vec<String> = store
        .graph
        .requirements
        .iter()
        .map(|(rid, r)| format!("- {}: {}", rid, r.statement))
        .collect();
    statements.sort();
    let mut body = String::new();
    for s in statements {
        if body.len() + s.len() > 12_000 {
            break;
        }
        body.push_str(&s);
        body.push('\n');
    }
    let entities: Vec<&str> = store
        .graph
        .entities
        .values()
        .map(|e| e.name.as_str())
        .take(40)
        .collect();
    // An existing deliverable pins the medium: a planted fixture or an earlier run
    // already chose the language and toolchain, and a guess from CLI-flavored
    // statements must not override it. "From the statements alone" applies only to
    // an empty deliverable.
    // Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
    let existing = list_existing_files(deliverable);
    let mut tree = if existing.is_empty() {
        STEPS.get("decide-tree-empty").to_string()
    } else {
        STEPS.fill("decide-tree-existing", &[("files", &existing.join("\n"))])
    };
    // Run commands recorded before any entity generated (bound tests) pin the
    // toolchain the same way an existing tree does: a re-decision after a divergence
    // must land on what the sessions actually wrote.
    // Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
    let mut commands: Vec<String> = Ledger::load(&store.out)
        .requirements
        .values()
        .filter(|r| r.test.kind == "programmatic" && !r.test.run.trim().is_empty())
        .map(|r| r.test.run.clone())
        .collect();
    commands.sort();
    commands.dedup();
    if !commands.is_empty() {
        tree.push_str("\n\n");
        tree.push_str(
            &STEPS.fill(
                "decide-commands",
                &[(
                    "commands",
                    &commands
                        .iter()
                        .take(10)
                        .map(|c| format!("- {}", c))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )],
            ),
        );
    }
    let system = STEPS.get("decide-system");
    let user = STEPS.fill(
        "decide-user",
        &[
            ("statements", &body),
            ("entities", &entities.join(", ")),
            ("tree", &tree),
        ],
    );
    let mut last = String::new();
    for attempt in 0..2 {
        let ask = if attempt == 0 {
            user.clone()
        } else {
            STEPS.fill("decide-retry", &[("request", &user), ("reason", &last)])
        };
        let reply = runner
            .ask(
                system,
                &ask,
                "gen medium",
                if attempt == 0 {
                    "decide"
                } else {
                    "decide retry"
                },
            )
            .await?;
        // A rate limit relayed as a reply is the endpoint's answer, not a decision:
        // say so instead of blaming an empty field (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
        if crate::reconcile::rate_limited(&reply) {
            return Err(format!(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/gen-medium/could-not-decide-the-deliverable-s-medium-902a3.md"
                )),
                crate::llm::truncate(reply.trim(), 200)
            ));
        }
        let excerpt = crate::llm::truncate(reply.trim(), 120).to_string();
        let raw = crate::llm::extract_json_object(&reply).unwrap_or(reply);
        let v: Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                last = format!("not JSON ({})", e);
                continue;
            }
        };
        let produced = v["produced"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        if produced != "written" && produced != "built" {
            last = format!(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/gen-medium/produced-was-not-written-or-built-the-57b0e.md"
                )),
                produced, excerpt
            );
            continue;
        }
        let artifact = v["artifact"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .trim_start_matches("./")
            .to_string();
        if produced == "built" && artifact.is_empty() {
            last = "`built` needs an `artifact` path".into();
            continue;
        }
        let form = v["form"].as_str().unwrap_or_default().trim().to_string();
        if form.is_empty() {
            last = "`form` was empty".into();
            continue;
        }
        return Ok(Medium {
            form,
            produced,
            toolchain: v["toolchain"]
                .as_str()
                .unwrap_or_default()
                .trim()
                .to_string(),
            artifact,
        });
    }
    Err(format!(
        "could not decide the deliverable's medium: {}",
        last
    ))
}

// What the decided medium demands of the file this task writes. The decision is
// already made; this states what follows from it, in the imperative.
pub(super) fn medium_directive(medium: &Option<Medium>) -> String {
    match medium {
        Some(m) if m.is_built() => STEPS.fill(
            "directive-built",
            &[
                ("form", &m.form),
                (
                    "toolchain",
                    if m.toolchain.is_empty() {
                        "source"
                    } else {
                        &m.toolchain
                    },
                ),
                ("artifact", &m.artifact),
            ],
        ),
        Some(m) => {
            let toolchain = if m.toolchain.is_empty() {
                String::new()
            } else {
                STEPS.fill(
                    "directive-written-toolchain",
                    &[("toolchain", &m.toolchain)],
                )
            };
            STEPS.fill(
                "directive-written",
                &[("form", &m.form), ("toolchain", &toolchain)],
            )
        }
        // No decision on record (an external worker's package): fall back to deriving it.
        None => STEPS.get("directive-undecided").to_string(),
    }
}
