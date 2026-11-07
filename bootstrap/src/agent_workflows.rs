// Shared conversational policy and deferred workflows for every agent transport.
// Mirrors docs/frontends/agent-workflows.md.
use std::path::Path;
use serde_json::{json, Value};

pub const TEMPLATE: &str = include_str!("../../docs/compiler/goals/prompts/agent-main.mustache");
pub const TOOLS: [&str; 2] = ["list_jazyk_skills", "read_jazyk_skill"];

pub struct Skill {
    pub name: &'static str,
    pub description: &'static str,
    pub capabilities: &'static str,
    pub source: &'static str,
    pub body: &'static str,
}

pub const SKILLS: &[Skill] = &[
    Skill { name: "inspect-project", description: "Inspect project questions, code, docs, reviews, and status without changing them.", capabilities: "read", source: "docs/compiler/skills/agent/inspect-project.md", body: include_str!("../../docs/compiler/skills/agent/inspect-project.md") },
    Skill { name: "edit-documentation", description: "Edit prose, requirements, criteria, or intended behavior without implicit compilation.", capabilities: "document writes", source: "docs/compiler/skills/agent/edit-documentation.md", body: include_str!("../../docs/compiler/skills/agent/edit-documentation.md") },
    Skill { name: "change-code", description: "Repair or change code within requirements; update docs first for authorized behavior changes.", capabilities: "source reads and writes, generation recording", source: "docs/compiler/skills/agent/change-code.md", body: include_str!("../../docs/compiler/skills/agent/change-code.md") },
    Skill { name: "compile", description: "Compile documentation into the semantic graph and resolve reconciliation goals.", capabilities: "compile orchestration or goal loop", source: "docs/compiler/skills/agent/compile.md", body: include_str!("../../docs/compiler/skills/agent/compile.md") },
    Skill { name: "generate", description: "Implement documented requirements and record deliverables and tests.", capabilities: "generation and source writes", source: "docs/compiler/skills/agent/generate.md", body: include_str!("../../docs/compiler/skills/agent/generate.md") },
    Skill { name: "bind", description: "Connect existing implementation and test evidence to requirements.", capabilities: "binding", source: "docs/compiler/skills/agent/bind.md", body: include_str!("../../docs/compiler/skills/agent/bind.md") },
    Skill { name: "verify", description: "Run tests or judge observable criteria and record current evidence.", capabilities: "verification or authorized test execution", source: "docs/compiler/skills/agent/verify.md", body: include_str!("../../docs/compiler/skills/agent/verify.md") },
    Skill { name: "reverse-compile", description: "Draft documentation from observed implementation and surface intent conflicts.", capabilities: "decompilation or authorized code inspection and drafting", source: "docs/compiler/skills/agent/reverse-compile.md", body: include_str!("../../docs/compiler/skills/agent/reverse-compile.md") },
    Skill { name: "execute-task", description: "Inspect and execute the requested task under its approval and cost rules.", capabilities: "task execution", source: "docs/compiler/skills/agent/execute-task.md", body: include_str!("../../docs/compiler/skills/agent/execute-task.md") },
    Skill { name: "recover-workflow", description: "Diagnose failed tools, conflicts, interrupted work, or missing prerequisites.", capabilities: "relevant inspection and repair tools", source: "docs/compiler/skills/agent/recover-workflow.md", body: include_str!("../../docs/compiler/skills/agent/recover-workflow.md") },
    Skill { name: "publish", description: "Push, publish, or deploy only with explicit authority and verified integration state.", capabilities: "publication integration and authority", source: "docs/compiler/skills/agent/publish.md", body: include_str!("../../docs/compiler/skills/agent/publish.md") },
];

// One line per set, from the project's configuration.
fn sets_text(workspace: &crate::document_sets::Workspace) -> String {
    if workspace.sets.is_empty() {
        return "No document sets yet. Add one in the GUI or with `update_project_settings`.".into();
    }
    let list = |patterns: &[String]| {
        let shown: Vec<String> = patterns
            .iter()
            .filter(|p| !p.starts_with('!'))
            .map(|p| format!("`{p}`"))
            .collect();
        let excluded = patterns.iter().filter(|p| p.starts_with('!')).count();
        let mut s = if shown.is_empty() {
            "none".to_string()
        } else {
            shown.join(", ")
        };
        if excluded > 0 {
            s.push_str(&format!(
                " ({excluded} exclusion{})",
                if excluded == 1 { "" } else { "s" }
            ));
        }
        s
    };
    let mut lines = vec!["Document sets:".to_string(), String::new()];
    for set in &workspace.sets {
        let mut line = format!("- `{}` ({}): docs {}", set.id, set.name.trim(), list(&set.docs));
        if !set.roots.is_empty() {
            line.push_str(&format!(", roots {}", list(&set.roots)));
        }
        line.push_str(&format!(
            ", source {}",
            if set.source.is_empty() {
                "none".to_string()
            } else {
                list(&set.source)
            }
        ));
        lines.push(line);
    }
    lines.join("\n")
}


// Both AGENTS.md and MCP initialize call this renderer with the same root.
pub fn render(root: &Path) -> Result<String, String> {
    let workspace = crate::document_sets::Workspace::load(root).map_err(|e| e.1)?;
    let skills = SKILLS.iter().map(|s| format!("- `{}`: {}", s.name, s.description))
        .collect::<Vec<_>>().join("\n");
    let template = mustache::compile_str(TEMPLATE).map_err(|e| format!("invalid agent main template: {e}"))?;
    template.render_to_string(&json!({"sets":sets_text(&workspace), "skills":skills}))
        .map_err(|e| format!("cannot render agent main template: {e}"))
}

pub fn catalog() -> Value {
    json!({"skills": SKILLS.iter().map(|s| json!({"name":s.name,
        "description":s.description, "requiredCapabilities":s.capabilities,
        "source":s.source})).collect::<Vec<_>>(),
        "note":"Read a skill before its workflow. Availability and authorization come from the serving and its gates, not from reading a skill."})
}

pub fn read(name: &str) -> Result<Value, String> {
    let skill = SKILLS.iter().find(|s| s.name == name).ok_or_else(|| format!(
        "unknown Jazyk skill `{name}`; one of {}", SKILLS.iter().map(|s| s.name).collect::<Vec<_>>().join(", ")))?;
    Ok(json!({"name":skill.name,"description":skill.description,
        "requiredCapabilities":skill.capabilities,"source":skill.source,"body":skill.body}))
}

pub fn tools() -> Vec<Value> {
    vec![
        json!({"name":"list_jazyk_skills","description":"List built-in deferred Jazyk workflow skills, their triggers, required capabilities, and source paths. Read-only. No graph or setId needed.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}}),
        json!({"name":"read_jazyk_skill","description":"Read the exact instructions of one built-in Jazyk workflow skill before using it. A skill grants no new tool, permission, or cost approval. No graph or setId needed.","inputSchema":{"type":"object","properties":{"name":{"type":"string","enum":SKILLS.iter().map(|s|s.name).collect::<Vec<_>>()}},"required":["name"],"additionalProperties":false}}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_and_exact_bodies_are_self_contained_and_resolve_every_route() {
        let names = SKILLS.iter().map(|s|s.name).collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names.len(), SKILLS.len());
        for skill in SKILLS {
            let reply = read(skill.name).unwrap();
            assert_eq!(reply["body"], skill.body);
            assert!(skill.body.starts_with("# "));
            for token in skill.body.split('`').skip(1).step_by(2) {
                if token.contains('-') && !token.contains(' ') && !token.contains('/') && !token.contains('*') {
                    assert!(names.contains(token), "unresolved skill reference `{token}` in {}", skill.name);
                }
            }
        }
        assert!(read("../../secret").unwrap_err().contains("unknown Jazyk skill"));
        assert!(TEMPLATE.contains("{{{sets}}}"));
        assert!(TEMPLATE.contains("{{{skills}}}"));
        assert!(TEMPLATE.contains("Never invent requirements"));
        assert!(TEMPLATE.contains("A docs edit does not authorize"));
    }
}
