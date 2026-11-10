use super::*;

// One call settles a batch: a consistent entry and a contradiction entry land
// (one resolution each, one diagnostic staged), the entry whose gate refuses it
// is reported in place with the rule and leaves nothing staged.
// Mirrors docs/compiler/tools.md#goal-tools.
#[test]
fn settle_pairs_lands_the_accepted_entries_and_reports_the_refused_one() {
    let mut t = pair_session();
    let v = t
        .dispatch(
            PAIR_SETTLE_TOOL,
            &json!({"verdicts": [
                {"goal": "g:rejudge-pair:req:a~req:b", "verdict": "consistent",
                 "justification": "Holding items and showing a count are different facts."},
                {"goal": "g:rejudge-pair:req:a~req:c", "verdict": "contradiction",
                 "severity": "error", "message": "held until checkout versus dropped after one hour",
                 "justification": "Both cannot hold: one keeps items until checkout, the other drops them after an hour."},
                {"goal": "g:rejudge-pair:req:b~req:c", "verdict": "resolved",
                 "justification": "The old finding lapsed."}
            ]}),
        )
        .unwrap();
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 3, "{}", v);
    assert_eq!(results[0]["verdict"], "consistent");
    assert!(results[0]["error"].is_null(), "{}", results[0]);
    assert_eq!(results[1]["verdict"], "contradiction");
    let d1 = results[1]["diagnostic"].as_str().unwrap().to_string();
    assert!(d1.starts_with("diag:contradiction-"), "{}", results[1]);
    assert_eq!(
        results[2]["error"]["rule"], "no-pair-diagnostic",
        "{}",
        results[2]
    );
    assert_eq!(
        v["settled"],
        json!(["g:rejudge-pair:req:a~req:b", "g:rejudge-pair:req:a~req:c"])
    );
    let open = v["open"].as_array().unwrap();
    assert!(
        open.iter().any(|g| g == "g:rejudge-pair:req:b~req:c"),
        "{}",
        v
    );
    // One resolution per goal with its own justification and evidence, as the
    // journal records them; one diagnostic staged for the contradiction.
    let commit = t.commit(1, None);
    assert_eq!(commit.resolved.len(), 2);
    let by_goal: BTreeMap<&str, &Resolved> = commit
        .resolved
        .iter()
        .map(|r| (r.goal.as_str(), r))
        .collect();
    assert_eq!(
        by_goal["g:rejudge-pair:req:a~req:b"].evidence,
        json!({"verdict": "consistent"})
    );
    assert_eq!(
        by_goal["g:rejudge-pair:req:a~req:c"].evidence["carried_by"],
        d1.as_str()
    );
    assert!(by_goal["g:rejudge-pair:req:a~req:c"]
        .justification
        .starts_with("Both cannot hold"));
    assert_eq!(t.staged.len(), 1);
    assert!(
        matches!(&t.staged[0], Op::ReportDiagnostic { diagnostic, .. }
        if diagnostic.rule == "contradiction" && diagnostic.severity == "error"
        && diagnostic.subjects == vec!["req:a".to_string(), "req:c".to_string()])
    );
    // A mutating reply previews and re-renders the status like any write.
    assert!(v["status"].is_string(), "{}", v);
}

// The other verdicts and the refusals: duplicate files and marks, resolved
// settles the pair's open diagnostic, a goal of another kind and a goal outside
// the batch are refused by name, a contradiction without its message is refused
// before anything stages, and the guard counts accepted entries.
// Mirrors docs/compiler/goals/rejudge-pair.md#tools: an entry may name carried_by,
// an open contradiction on one side stating the same conflict, and nothing new files.
#[test]
fn settle_pairs_carried_by_reuses_a_one_sided_contradiction() {
    let mut t = pair_session();
    t.snapshot.graph.diagnostics.insert(
        "diag:contradiction-5".into(),
        Diagnostic {
            rule: "contradiction".into(),
            severity: "error".into(),
            subjects: vec!["req:c".into(), "req:zzz".into()],
            message: "500 versus 250".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        },
    );
    let v = t
        .dispatch(
            PAIR_SETTLE_TOOL,
            &json!({"verdicts": [
                {"goal": "g:rejudge-pair:req:a~req:c", "verdict": "contradiction", "carried_by": "diag:contradiction-5",
                 "justification": "The same threshold conflict diag:contradiction-5 already states against req:c."}
            ]}),
        )
        .unwrap();
    let r = &v["results"][0];
    assert!(r["error"].is_null(), "{}", r);
    assert_eq!(r["diagnostic"], "diag:contradiction-5", "{}", r);
    assert!(t.staged_diags.is_empty());
}

// Mirrors docs/compiler/goals/rejudge-pair.md#tools: a contradiction already
// standing on the pair carries the verdict; the entry needs no severity or
// message, and nothing new is filed.
#[test]
fn settle_pairs_lets_a_standing_contradiction_carry_the_verdict() {
    let mut t = pair_session();
    t.snapshot.graph.diagnostics.insert(
        "diag:contradiction-3".into(),
        Diagnostic {
            rule: "contradiction".into(),
            severity: "error".into(),
            subjects: vec!["req:a".into(), "req:c".into()],
            message: "filed before the pair opened".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        },
    );
    let v = t
        .dispatch(
            PAIR_SETTLE_TOOL,
            &json!({"verdicts": [
                {"goal": "g:rejudge-pair:req:a~req:c", "verdict": "contradiction",
                 "justification": "The standing finding still holds: one keeps items until checkout, the other drops them after an hour."}
            ]}),
        )
        .unwrap();
    let r = &v["results"][0];
    assert!(r["error"].is_null(), "{}", r);
    assert_eq!(r["diagnostic"], "diag:contradiction-3", "{}", r);
    assert!(
        r["note"].as_str().unwrap().contains("already stood"),
        "{}",
        r
    );
    assert!(t.staged_diags.is_empty(), "{:?}", t.staged_diags.keys());
    assert_eq!(v["settled"], json!(["g:rejudge-pair:req:a~req:c"]));
}

#[test]
fn settle_pairs_duplicate_resolved_and_the_per_entry_guard() {
    let mut t = pair_session();
    // A duplicate across documents is the filing case; the same-document case
    // is a delete and is refused below.
    if let Some(src) = t
        .snapshot
        .graph
        .requirements
        .get_mut("req:a")
        .and_then(|r| r.source.as_mut())
    {
        src.doc = "other.md".into();
    }
    t.snapshot.graph.diagnostics.insert(
        "diag:contradiction-7".into(),
        Diagnostic {
            rule: "contradiction".into(),
            severity: "warning".into(),
            subjects: vec!["req:b".into(), "req:c".into()],
            message: "old finding".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        },
    );
    let v = t
        .dispatch(
            PAIR_SETTLE_TOOL,
            &json!({"verdicts": [
                {"goal": "g:rejudge-pair:req:a~req:b", "verdict": "duplicate",
                 "justification": "The same obligation stated twice."},
                {"goal": "g:rejudge-pair:req:b~req:c", "verdict": "resolved",
                 "justification": "The count and the timeout no longer conflict."},
                {"goal": "g:reconcile-section:shop.md#/shop", "verdict": "consistent",
                 "justification": "Not a pair."},
                {"goal": "g:rejudge-pair:req:x~req:y", "verdict": "consistent",
                 "justification": "Not in the batch."},
                {"goal": "g:rejudge-pair:req:a~req:c", "verdict": "contradiction",
                 "severity": "error", "justification": "No message given."},
                {"goal": "g:rejudge-pair:req:a~req:c", "verdict": "contradiction",
                 "severity": "info", "message": "x", "justification": "Wrong severity."}
            ]}),
        )
        .unwrap();
    let r = v["results"].as_array().unwrap();
    let dup = r[0]["diagnostic"].as_str().unwrap().to_string();
    assert!(dup.starts_with("diag:duplicate-requirement-"), "{}", r[0]);
    assert_eq!(
        r[1]["resolved"],
        json!(["diag:contradiction-7"]),
        "{}",
        r[1]
    );
    assert_eq!(r[2]["error"]["rule"], "wrong-kind", "{}", r[2]);
    assert_eq!(r[3]["error"]["rule"], "unknown-goal", "{}", r[3]);
    assert_eq!(r[4]["error"]["rule"], "bad-args", "{}", r[4]);
    assert!(r[4]["error"]["message"]
        .as_str()
        .unwrap()
        .contains("message"));
    // Two sides quoting one document: the delete route, never a filing.
    let same = t
        .dispatch(
            PAIR_SETTLE_TOOL,
            &json!({"verdicts": [
                {"goal": "g:rejudge-pair:req:b~req:c", "verdict": "duplicate",
                 "justification": "Restated in one document."}
            ]}),
        )
        .unwrap();
    assert_eq!(
        same["results"][0]["error"]["rule"], "same-document-duplicate",
        "{}",
        same
    );
    assert_eq!(r[5]["error"]["rule"], "bad-severity", "{}", r[5]);
    // Two staged ops: the duplicate filing and the resolve; the refused
    // contradiction staged nothing.
    assert_eq!(t.staged.len(), 2, "{:?}", t.staged);
    assert!(
        matches!(&t.staged[0], Op::ReportDiagnostic { diagnostic, .. }
        if diagnostic.rule == "duplicate-requirement" && diagnostic.severity == "info")
    );
    assert!(
        matches!(&t.staged[1], Op::ResolveDiagnostic { id, .. } if id == "diag:contradiction-7")
    );
    // The duplicate resolution carries the diagnostic that carried it.
    let commit = t.commit(1, None);
    assert_eq!(commit.resolved.len(), 2);
    // A verbatim re-send of an accepted entry is marked, then refused; the
    // re-report answers with the same id and stages an update, never a second finding.
    let again = json!({"verdicts": [
        {"goal": "g:rejudge-pair:req:a~req:b", "verdict": "duplicate",
         "justification": "The same obligation stated twice."}
    ]});
    let v = t.dispatch(PAIR_SETTLE_TOOL, &again).unwrap();
    assert!(v["results"][0]["repeat"].is_string(), "{}", v);
    assert_eq!(v["results"][0]["diagnostic"], dup.as_str());
    let v = t.dispatch(PAIR_SETTLE_TOOL, &again).unwrap();
    assert_eq!(v["results"][0]["error"]["rule"], "repeated-call", "{}", v);
    assert!(v["settled"].as_array().unwrap().is_empty());
    // The whole call is exempt from the call-level guard: the same list sent
    // three times is never refused as a call.
    assert!(t.dispatch(PAIR_SETTLE_TOOL, &again).is_ok());
    // An empty list is a bad call.
    let err = t
        .dispatch(PAIR_SETTLE_TOOL, &json!({"verdicts": []}))
        .unwrap_err();
    assert_eq!(err.rule, "bad-args");
}
