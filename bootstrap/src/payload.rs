// Model-facing text that is not a whole payload file of its own: the fragment files
// under docs/compiler/goals/prompts/ and the placeholder fill shared by every payload.
// Mirrors docs/compiler/sessions.md#payload-files.

use regex::{Captures, Regex};
use std::sync::OnceLock;

// A `{name}` placeholder: lowercase letters, digits, and underscores. Literal braces
// in a payload (`{"batch": ...}`, `{goal, justification}`) never match.
fn placeholder() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{([a-z][a-z0-9_]*)\}").unwrap())
}

// The placeholder names a template carries, in order of first appearance.
pub fn placeholders(template: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for c in placeholder().captures_iter(template) {
        let n = c[1].to_string();
        if !names.contains(&n) {
            names.push(n);
        }
    }
    names
}

// Fill every `{name}` in one pass, so a value that itself holds `{text}` (a quote, a
// user's reply) is never filled again. A placeholder without a value stays as it is;
// a debug build refuses a template and a value list that disagree.
pub fn fill(template: &str, vars: &[(&str, &str)]) -> String {
    debug_assert!(
        {
            let mut want = placeholders(template);
            want.sort();
            let mut have: Vec<String> = vars.iter().map(|(k, _)| k.to_string()).collect();
            have.sort();
            have.dedup();
            want == have
        },
        "payload placeholders {:?} do not match the values {:?}",
        placeholders(template),
        vars.iter().map(|(k, _)| *k).collect::<Vec<_>>()
    );
    placeholder()
        .replace_all(template, |c: &Captures| {
            vars.iter()
                .find(|(k, _)| *k == &c[1])
                .map(|(_, v)| v.to_string())
                .unwrap_or_else(|| c[0].to_string())
        })
        .into_owned()
}

// A fragment file: named sections, each opened by a marker line `<!-- name -->`. A
// section's text runs to the next marker or the end of the file, trailing newlines
// dropped, so blank lines may separate sections. Markers never reach the model.
pub struct Fragments(pub &'static str);

impl Fragments {
    // Every section, in file order.
    pub fn sections(&self) -> Vec<(&'static str, &'static str)> {
        let text: &'static str = self.0;
        let mut out: Vec<(&'static str, &'static str)> = Vec::new();
        let mut open: Option<(&'static str, usize)> = None;
        let mut at = 0usize;
        for line in text.split_inclusive('\n') {
            let bare = line.trim_end_matches('\n');
            if let Some(name) = bare
                .strip_prefix("<!-- ")
                .and_then(|r| r.strip_suffix(" -->"))
                .filter(|n| !n.is_empty() && !n.contains(' '))
            {
                if let Some((n, start)) = open.take() {
                    out.push((n, text[start..at].trim_end_matches('\n')));
                }
                open = Some((name, at + line.len()));
            }
            at += line.len();
        }
        if let Some((n, start)) = open {
            out.push((n, text[start..].trim_end_matches('\n')));
        }
        out
    }

    // One section's text. A missing name is a build defect the payload table test
    // catches, so it panics.
    pub fn get(&self, name: &str) -> &'static str {
        self.sections()
            .into_iter()
            .find(|(n, _)| *n == name)
            .map(|(_, t)| t)
            .unwrap_or_else(|| panic!("payload section `{}` is missing", name))
    }

    // One section, filled.
    pub fn fill(&self, name: &str, vars: &[(&str, &str)]) -> String {
        fill(self.get(name), vars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_is_one_pass_and_keeps_literal_braces() {
        let t = "PROTOCOL {target}: {\"batch\": \"{target}\"} ({goal, justification}) {quote}";
        let s = fill(t, &[("target", "b1-0"), ("quote", "{target}")]);
        assert_eq!(
            s,
            "PROTOCOL b1-0: {\"batch\": \"b1-0\"} ({goal, justification}) {target}"
        );
        assert_eq!(placeholders(t), vec!["target", "quote"]);
    }

    #[test]
    fn fragments_split_on_markers_and_drop_trailing_newlines() {
        let f = Fragments("<!-- a -->\nfirst\nline\n\n<!-- b -->\n\nsecond\n");
        assert_eq!(f.sections(), vec![("a", "first\nline"), ("b", "\nsecond")]);
        assert_eq!(f.get("b"), "\nsecond");
    }

    // Every fragment section the code renders, with the placeholders the code fills.
    // A payload edit that adds, drops, or renames a section or a placeholder fails
    // here, and every section filled leaves no placeholder behind.
    // Mirrors docs/compiler/sessions.md#payload-files.
    #[allow(clippy::type_complexity)]
    const TABLE: &[(&str, &Fragments, &[(&str, &[&str])])] = &[
        (
            "gates.md",
            &crate::session::GATES,
            &[
            ("place-anchors", &[]),
            ("reconcile-section", &[]),
            ("rejudge-pair", &[]),
            ("review-entity", &[]),
            ("derive-criteria", &[]),
            ("retrace", &[]),
            ("conform-instance", &[]),
            ("bind", &[]),
            ("generate", &[]),
            ("verify", &[]),
            ("ratify", &[]),
            ("answer", &[]),
            ("declare-edges", &[]),
            ("dedupe-candidates", &[]),
            ("curate-view", &[]),
            ("split-view", &[]),
            ("abstract-entity", &[]),
            ("unknown", &[]),
            ],
        ),
        (
            "session-blocks.md",
            &crate::session::BLOCKS,
            &[
            ("contract-elided", &[]),
            ("skill-active", &["name"]),
            ("skill-delivered", &["name"]),
            ("project", &[]),
            ("project-generation", &["generation", "mode"]),
            ("project-diagnostics-none", &[]),
            ("project-diagnostics", &["counts", "ratification"]),
            ("project-diagnostic-count", &["count", "severity"]),
            ("project-diagnostics-ratification", &["count"]),
            ("project-board", &["here", "elsewhere", "blocked", "ratification"]),
            ("project-board-ratification", &["count"]),
            ("project-lint", &["rules"]),
            ("project-lint-rule", &["index", "severity", "rule"]),
            ("task-execution", &[]),
            ("goals", &[]),
            ("goal", &["id", "necessity"]),
            ("goal-contract-pointer", &["kind"]),
            ("goal-change", &["change"]),
            ("goal-gate", &["gate"]),
            ("goal-hints", &["hints"]),
            ("loaded-items", &[]),
            ],
        ),
        (
            "agent-context.md",
            &crate::agent_context::FRAMING,
            &[
            ("project-instructions", &["files"]),
            ("instruction-file", &["name", "text"]),
            ("instructions-cut", &[]),
            ("global-instructions", &["label", "text"]),
            ("skills", &[]),
            ("skill", &["name", "description"]),
            ("read-skill-note", &[]),
            ("warnings", &["warnings"]),
            ("warning", &["warning"]),
            ("remote-server", &["name", "instructions"]),
            ],
        ),
        (
            "serving-instructions.md",
            &crate::mcp::SERVING,
            &[
            ("initialize", &["policy", "capabilities"]),
            ("project-unloadable", &["config", "error"]),
            ("toolsets", &["modes"]),
            ("initialized", &[]),
            ("uninitialized", &[]),
            ("tab", &[]),
            ("writer-build-note", &[]),
            ("graph-write", &[]),
            ("workflow", &[]),
            ("reader", &[]),
            ("tab-build-note", &[]),
            ("begin-goals-next", &[]),
            ("compile-incomplete", &["failed"]),
            ("compile-converged", &[]),
            ("optional-ready", &["count"]),
            ("generation-ready", &["count"]),
            ("nothing-pending", &[]),
            ("next-batch", &["batch"]),
            ("no-batch-ready", &[]),
            ("began-note", &[]),
            ("ledger-nothing-owed", &[]),
            ("ledger-gated", &["begin"]),
            ("ledger-ready", &["begin"]),
            ("ledger-stale", &["count"]),
            ("source-mapping", &[]),
            ],
        ),
        (
            "generation-steps.md",
            &crate::gen::STEPS,
            &[
            ("medium-line-built", &["form", "toolchain", "artifact"]),
            ("medium-line-written", &["form", "toolchain"]),
            ("medium-undecided", &[]),
            ("decide-tree-empty", &[]),
            ("decide-tree-existing", &["files"]),
            ("decide-commands", &["commands"]),
            ("decide-system", &[]),
            ("decide-user", &["statements", "entities", "tree"]),
            ("decide-retry", &["request", "reason"]),
            ("directive-built", &["form", "toolchain", "artifact"]),
            ("directive-written", &["form", "toolchain"]),
            ("directive-written-toolchain", &["toolchain"]),
            ("directive-undecided", &[]),
            ("header", &["medium", "build_error", "id", "name", "context", "changed", "group", "generated", "commands", "build"]),
            ("build-broken", &["error", "whose"]),
            ("build-broken-elsewhere", &[]),
            ("build-broken-yours", &["files"]),
            ("header-group", &["group"]),
            ("header-commands-none", &[]),
            ("header-build-none", &[]),
            ("requirement", &["id", "test", "statement", "quote"]),
            ("product-first", &["header", "directive", "parts", "requirements"]),
            ("product-more", &["header", "group", "parts", "requirements", "path", "code"]),
            ("product-format-retry", &["request", "error"]),
            ("owner-retry", &["request", "path", "owner"]),
            ("tests", &["header", "product", "artifact_rule", "requirements", "code"]),
            ("tests-built", &["artifact"]),
            ("tests-default", &[]),
            ("tests-format-retry", &["request", "error"]),
            ("manifest", &["id", "files", "deliverable", "commands", "build", "entry", "names", "build_rule", "requirements", "tests", "tests_code"]),
            ("manifest-commands-none", &[]),
            ("manifest-build-none", &[]),
            ("manifest-entry-none", &[]),
            ("manifest-entry", &["path", "content"]),
            ("manifest-names-none", &[]),
            ("manifest-build-required", &["form", "artifact"]),
            ("manifest-build-omit", &[]),
            ("manifest-build-reuse-built", &["artifact"]),
            ("manifest-build-reuse", &[]),
            ("manifest-requirement", &["id", "test"]),
            ("manifest-format-retry", &["request", "error"]),
            ("mismatch-undeclared", &["id", "test", "tests"]),
            ("mismatch-absent", &["id", "test"]),
            ("mismatch-entry", &["entry", "artifact", "missing"]),
            ("mismatch-build", &["form", "artifact"]),
            ("manifest-retry", &["request", "answer", "problems"]),
            ("llm-criteria", &["id", "hash", "statement", "quote", "files"]),
            ],
        ),
        (
            "judge-input.md",
            &crate::verify::JUDGE,
            &[
            ("input", &["criterion", "criteria", "context", "files", "ask"]),
            ("criterion", &["statement"]),
            ("file", &["path", "text"]),
            ("ask-criterion", &[]),
            ("ask-test", &["test"]),
            ("ask-row", &[]),
            ],
        ),
        (
            "decompile-worker.md",
            &crate::decompile::WORKER,
            &[
            ("system", &["contract"]),
            ("scope", &["scope", "path", "lint", "inventory"]),
            ("retry", &["request", "reason"]),
            ],
        ),
        (
            "answer-handling.md",
            &crate::answer::HANDLING,
            &[
            ("heading", &["id", "rule", "severity", "finding"]),
            ("question", &["question"]),
            ("answer", &["answer"]),
            ("subjects", &[]),
            ("subject-quoted", &["id", "statement", "doc", "section", "quote"]),
            ("subject-derived", &["id", "statement", "provenance"]),
            ("subject-entity", &["id", "name", "definition"]),
            ("subject", &["id"]),
            ("act", &["id"]),
            ],
        ),
    ];

    // The whole-file payloads that carry placeholders.
    const FILES: &[(&str, &str, &[&str])] = &[
        (
            "agent-contract.md",
            include_str!("../../docs/compiler/goals/prompts/agent-contract.md"),
            &["feedback"],
        ),
        (
            "worker-protocol.md",
            include_str!("../../docs/compiler/goals/prompts/worker-protocol.md"),
            &["target"],
        ),
        (
            "serving-protocol.md",
            include_str!("../../docs/compiler/goals/prompts/serving-protocol.md"),
            &["target"],
        ),
        (
            "open-batch-protocol.md",
            include_str!("../../docs/compiler/goals/prompts/open-batch-protocol.md"),
            &["target"],
        ),
        ("integration-context.md", crate::project_context::INSTRUCTIONS, &["context"]),
    ];

    #[test]
    fn every_payload_renders_with_its_placeholders_filled_and_none_left() {
        let check = |what: &str, text: &str, want: &[&str]| {
            assert_eq!(placeholders(text), want, "{} placeholders", what);
            let vars: Vec<(&str, &str)> = want.iter().map(|n| (*n, "value")).collect();
            let filled = fill(text, &vars);
            assert!(placeholders(&filled).is_empty(), "{} left a placeholder: {}", what, filled);
            assert!(!filled.contains("<!-- "), "{} carries a section marker", what);
        };
        for (file, fragments, sections) in TABLE {
            let have: Vec<&str> = fragments.sections().iter().map(|(n, _)| *n).collect();
            let want: Vec<&str> = sections.iter().map(|(n, _)| *n).collect();
            assert_eq!(have, want, "{} sections", file);
            for (name, vars) in sections.iter() {
                let text = fragments.get(name);
                assert!(!text.trim().is_empty(), "{}#{} is empty", file, name);
                check(&format!("{}#{}", file, name), text, vars);
            }
        }
        for (file, text, vars) in FILES {
            check(file, text, vars);
        }
    }

    // Every goal kind, the two human-blocked ones included, has its own gate line.
    #[test]
    fn every_goal_kind_has_a_gate_line() {
        let gates = crate::session::GATES;
        for k in crate::goals::REGISTRY.iter() {
            assert!(
                gates.sections().iter().any(|(n, _)| *n == k.kind()),
                "gates.md has no line for {}",
                k.kind()
            );
            assert_ne!(crate::session::gate_line(k.kind()), gates.get("unknown"));
        }
    }
}
