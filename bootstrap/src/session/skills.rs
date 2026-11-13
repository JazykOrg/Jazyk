use super::*;

// The skill payloads, embedded at compile time. Mirrors docs/compiler/sessions.md#skills.
pub const SKILLS: [(&str, &str); 8] = [
    (
        "extraction",
        include_str!(concat!(env!("OUT_DIR"), "/payloads/skills/extraction.md")),
    ),
    (
        "edges",
        include_str!(concat!(env!("OUT_DIR"), "/payloads/skills/edges.md")),
    ),
    (
        "judgment",
        include_str!(concat!(env!("OUT_DIR"), "/payloads/skills/judgment.md")),
    ),
    (
        "criteria",
        include_str!(concat!(env!("OUT_DIR"), "/payloads/skills/criteria.md")),
    ),
    (
        "flow-views",
        include_str!(concat!(env!("OUT_DIR"), "/payloads/skills/flow-views.md")),
    ),
    (
        "structural-views",
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/skills/structural-views.md"
        )),
    ),
    (
        "abstraction",
        include_str!(concat!(env!("OUT_DIR"), "/payloads/skills/abstraction.md")),
    ),
    (
        "conformance",
        include_str!(concat!(env!("OUT_DIR"), "/payloads/skills/conformance.md")),
    ),
];

pub fn skill_payload(name: &str) -> Option<&'static str> {
    SKILLS.iter().find(|(n, _)| *n == name).map(|(_, p)| *p)
}

// The skill a loaded node auto-activates: a section brings extraction, a flow view
// flow-views, a structural view structural-views. Mirrors docs/compiler/sessions.md#skills.
pub fn skill_for_target(store: &Store, target: &str) -> Option<&'static str> {
    if crate::model::split_section_ref(target).is_some() || store.docs.contains_key(target) {
        return Some("extraction");
    }
    if let Some(v) = store.graph.views.get(target) {
        return Some(if goals::is_flow_kind(&v.kind) {
            "flow-views"
        } else {
            "structural-views"
        });
    }
    None
}

// ---- skills state ----

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SkillEntry {
    pub name: String,
    pub active: bool,
    pub chars: usize,
}

// What one activation did. Rendered carries the payload exactly once.
pub enum SkillLoad {
    Rendered(&'static str),
    Active,
    Reactivated,
    CapReached,
}

// The skills rendered this session, active or inactive, capped. Rendered text stays
// in the conversation, so the cap counts every rendering and an inactive skill keeps
// its chars. Mirrors docs/compiler/sessions.md#skills.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillState {
    pub rendered: Vec<SkillEntry>,
    pub cap: usize,
    // Goal-kind skills: active from the first round, never deactivated while the
    // session runs.
    pinned: BTreeSet<String>,
}

impl Default for SkillState {
    fn default() -> Self {
        SkillState::new()
    }
}

impl SkillState {
    pub fn new() -> SkillState {
        SkillState {
            rendered: Vec::new(),
            cap: crate::limits::SKILLS_PER_SESSION,
            pinned: BTreeSet::new(),
        }
    }

    // Whether a goal kind's skill governs this session. A batch works under its
    // kinds' skills and reads other nodes as reference; only a serving auto-loads.
    // Mirrors docs/compiler/sessions.md#skills.
    pub fn has_pinned(&self) -> bool {
        !self.pinned.is_empty()
    }

    // A goal kind's skill: rendered in the prompt from the first round, pinned active.
    pub fn pin(&mut self, name: &str) {
        if skill_payload(name).is_none() {
            return;
        }
        self.pinned.insert(name.to_string());
        if !self.rendered.iter().any(|e| e.name == name) && self.rendered.len() < self.cap {
            self.rendered.push(SkillEntry {
                name: name.to_string(),
                active: true,
                chars: skill_payload(name).map(crate::context::chars).unwrap_or(0),
            });
        }
    }

    // Bring a skill in: renders once, reactivates without re-rendering, refuses past
    // the cap. An unknown name errors with the index.
    pub fn activate(&mut self, name: &str) -> Result<SkillLoad, String> {
        let Some(payload) = skill_payload(name) else {
            return Err(format!(
                "unknown skill `{}`; the skills: {}",
                name,
                SKILLS
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        };
        if let Some(e) = self.rendered.iter_mut().find(|e| e.name == name) {
            if e.active {
                return Ok(SkillLoad::Active);
            }
            e.active = true;
            return Ok(SkillLoad::Reactivated);
        }
        if self.rendered.len() >= self.cap {
            return Ok(SkillLoad::CapReached);
        }
        self.rendered.push(SkillEntry {
            name: name.to_string(),
            active: true,
            chars: crate::context::chars(payload),
        });
        Ok(SkillLoad::Rendered(payload))
    }

    // Unloading the last node of a kind marks its skill inactive; the text already in
    // context stands and keeps its chars and its cap slot.
    pub fn deactivate(&mut self, name: &str) {
        if self.pinned.contains(name) {
            return;
        }
        if let Some(e) = self.rendered.iter_mut().find(|e| e.name == name) {
            e.active = false;
        }
    }

    // A skill shipped as a pointer was never rendered: drop the entry so load_skill
    // renders it. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    pub fn forget(&mut self, name: &str) {
        self.rendered.retain(|e| e.name != name);
    }

    pub fn is_active(&self, name: &str) -> bool {
        self.rendered.iter().any(|e| e.name == name && e.active)
    }

    pub fn is_rendered(&self, name: &str) -> bool {
        self.rendered.iter().any(|e| e.name == name)
    }

    pub fn rendered_chars(&self) -> usize {
        self.rendered.iter().map(|e| e.chars).sum()
    }

    pub fn rendered_names(&self) -> Vec<String> {
        self.rendered.iter().map(|e| e.name.clone()).collect()
    }

    // The active payloads, prompt order.
    pub fn active_payloads(&self) -> Vec<(String, &'static str)> {
        self.rendered
            .iter()
            .filter(|e| e.active)
            .filter_map(|e| skill_payload(&e.name).map(|p| (e.name.clone(), p)))
            .collect()
    }

    // The index line under ## Loaded: active skills with their size, rendered but
    // inactive ones marked, the rest loadable.
    pub fn index_line(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for e in &self.rendered {
            if e.active {
                parts.push(format!(
                    "{} (active, {:.1}k)",
                    e.name,
                    e.chars as f64 / 1000.0
                ));
            } else {
                parts.push(format!("{} (inactive)", e.name));
            }
        }
        let loadable: Vec<&str> = SKILLS
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| !self.rendered.iter().any(|e| e.name == *n))
            .collect();
        let mut line = format!("Skills: {}", parts.join("; "));
        if !loadable.is_empty() {
            if self.rendered.len() >= self.cap {
                line.push_str(&format!("; {} (cap reached)", loadable.join(", ")));
            } else {
                line.push_str(&format!("; {} (load_skill)", loadable.join(", ")));
            }
        }
        line
    }
}
