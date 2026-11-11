use super::*;

// Goal kinds pin their skills into a fresh session before anything else renders,
// so the cap can never drop a pinned skill while every skill fits under it.
// Mirrors docs/compiler/sessions.md#skills.
#[test]
fn every_pinned_skill_fits_under_the_session_cap() {
    assert!(SKILLS.len() <= crate::limits::SKILLS_PER_SESSION);
    let mut all = SkillState::new();
    for (name, _) in SKILLS {
        all.pin(name);
    }
    assert!(SKILLS.iter().all(|(name, _)| all.is_active(name)));
}

// Auto-load once, the cap, and the inactive marking.
// Mirrors docs/compiler/sessions.md#skills.
#[test]
fn skill_state_renders_once_caps_and_goes_inactive() {
    let mut sk = SkillState::new();
    sk.cap = 4; // Exercise a deliberately small fixture cap, independent of the production default.
    assert!(matches!(
        sk.activate("extraction"),
        Ok(SkillLoad::Rendered(_))
    ));
    assert!(matches!(sk.activate("extraction"), Ok(SkillLoad::Active)));
    sk.deactivate("extraction");
    assert!(!sk.is_active("extraction"));
    assert!(sk.index_line().contains("extraction (inactive)"));
    // Rendered text keeps its chars and its cap slot.
    assert!(sk.rendered_chars() > 0);
    assert!(matches!(
        sk.activate("extraction"),
        Ok(SkillLoad::Reactivated)
    ));
    for name in ["judgment", "flow-views", "structural-views"] {
        assert!(matches!(sk.activate(name), Ok(SkillLoad::Rendered(_))));
    }
    assert!(matches!(
        sk.activate("abstraction"),
        Ok(SkillLoad::CapReached)
    ));
    assert!(sk.index_line().contains("cap reached"));
    assert!(sk.activate("no-such-skill").is_err());
    // A pinned goal-kind skill never deactivates.
    let mut pinned = SkillState::new();
    pinned.pin("judgment");
    pinned.deactivate("judgment");
    assert!(pinned.is_active("judgment"));
}
