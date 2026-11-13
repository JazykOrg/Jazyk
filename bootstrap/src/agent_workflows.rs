// Shared conversational policy and deferred workflows for every agent transport.
// Mirrors docs/frontends/agent-workflows.md.
use serde_json::{json, Value};
use std::path::Path;

pub const TEMPLATE: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/payloads/goals/prompts/conversation/agent-main.md.mustache"
));
pub const TOOLS: [&str; 2] = ["list_jazyk_skills", "read_jazyk_skill"];

// What a skill-gated chat catalog always lists; each workflow skill's front
// matter lists the rest under `tools`.
// Mirrors docs/frontends/agent-workflows.md#skill-gated-tools.
pub const CORE_TOOLS: [&str; 23] = [
    "project_context",
    // Served only where no project exists, as the way to create one.
    "init_project",
    "list_jazyk_skills",
    "read_jazyk_skill",
    "load",
    "expand",
    "unload",
    "graph_status",
    "search",
    "read_section",
    "get_entity",
    "get_view",
    "diagnostics",
    "inspect_input",
    "read_text_file",
    "write_text_file",
    "list_files",
    "search_files",
    "read_document",
    "write_document",
    "list_document_sets",
    "recent_edits",
    "report_feedback",
];

// The tools one workflow skill's front matter lists, or None for an unknown skill.
pub fn skill_tools(name: &str) -> Option<Vec<String>> {
    let skill = SKILLS.iter().find(|s| s.name == name)?;
    let metadata = skill.metadata().ok()?;
    Some(
        metadata["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_str().map(str::to_owned))
            .collect(),
    )
}

pub struct Skill {
    pub name: &'static str,
    pub source: &'static str,
    pub body: &'static str,
    pub prepared: &'static str,
}

pub const SKILLS: &[Skill] = &[
    Skill {
        name: "inspect-project",
        source: "docs/compiler/skills/agent/inspect-project.md",
        body: include_str!("../../docs/compiler/skills/agent/inspect-project.md"),
        prepared: include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/skills/agent/inspect-project.md"
        )),
    },
    Skill {
        name: "edit-documentation",
        source: "docs/compiler/skills/agent/edit-documentation.md",
        body: include_str!("../../docs/compiler/skills/agent/edit-documentation.md"),
        prepared: include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/skills/agent/edit-documentation.md"
        )),
    },
    Skill {
        name: "change-code",
        source: "docs/compiler/skills/agent/change-code.md",
        body: include_str!("../../docs/compiler/skills/agent/change-code.md"),
        prepared: include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/skills/agent/change-code.md"
        )),
    },
    Skill {
        name: "compile",
        source: "docs/compiler/skills/agent/compile.md",
        body: include_str!("../../docs/compiler/skills/agent/compile.md"),
        prepared: include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/skills/agent/compile.md"
        )),
    },
    Skill {
        name: "generate",
        source: "docs/compiler/skills/agent/generate.md",
        body: include_str!("../../docs/compiler/skills/agent/generate.md"),
        prepared: include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/skills/agent/generate.md"
        )),
    },
    Skill {
        name: "bind",
        source: "docs/compiler/skills/agent/bind.md",
        body: include_str!("../../docs/compiler/skills/agent/bind.md"),
        prepared: include_str!(concat!(env!("OUT_DIR"), "/payloads/skills/agent/bind.md")),
    },
    Skill {
        name: "verify",
        source: "docs/compiler/skills/agent/verify.md",
        body: include_str!("../../docs/compiler/skills/agent/verify.md"),
        prepared: include_str!(concat!(env!("OUT_DIR"), "/payloads/skills/agent/verify.md")),
    },
    Skill {
        name: "reverse-compile",
        source: "docs/compiler/skills/agent/reverse-compile.md",
        body: include_str!("../../docs/compiler/skills/agent/reverse-compile.md"),
        prepared: include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/skills/agent/reverse-compile.md"
        )),
    },
    Skill {
        name: "execute-task",
        source: "docs/compiler/skills/agent/execute-task.md",
        body: include_str!("../../docs/compiler/skills/agent/execute-task.md"),
        prepared: include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/skills/agent/execute-task.md"
        )),
    },
    Skill {
        name: "recover-workflow",
        source: "docs/compiler/skills/agent/recover-workflow.md",
        body: include_str!("../../docs/compiler/skills/agent/recover-workflow.md"),
        prepared: include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/skills/agent/recover-workflow.md"
        )),
    },
    Skill {
        name: "publish",
        source: "docs/compiler/skills/agent/publish.md",
        body: include_str!("../../docs/compiler/skills/agent/publish.md"),
        prepared: include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/skills/agent/publish.md"
        )),
    },
];

// One line per set, from the project's configuration.
fn sets_text(workspace: &crate::document_sets::Workspace) -> String {
    if workspace.sets.is_empty() {
        return String::new();
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
        let mut line = format!(
            "- `{}` ({}): docs {}",
            set.id,
            set.name.trim(),
            list(&set.docs)
        );
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
    crate::payload::render_mustache(TEMPLATE, &json!({"sets":sets_text(&workspace)}))
}

fn tool_payload() -> Value {
    serde_json::from_str(include_str!(
        "../../docs/compiler/goals/prompts/conversation/agent-skill-tools.json"
    ))
    .expect("valid embedded workflow skill tool definitions")
}

impl Skill {
    fn metadata(&self) -> Result<Value, String> {
        let (mut metadata, _) = crate::agent_context::front_matter(self.body)
            .ok_or_else(|| format!("invalid built-in skill front matter: {}", self.source))?;
        if metadata["name"] != self.name
            || !["description", "requiredCapabilities"]
                .iter()
                .all(|key| metadata[*key].as_str().is_some_and(|s| !s.is_empty()))
        {
            return Err(format!("invalid built-in skill metadata: {}", self.source));
        }
        metadata["source"] = json!(self.source);
        Ok(metadata)
    }
}

pub fn catalog() -> Result<Value, String> {
    let skills = SKILLS
        .iter()
        .map(Skill::metadata)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"skills":skills,"note":tool_payload()["note"]}))
}

pub fn read(name: &str) -> Result<Value, String> {
    let skill = SKILLS.iter().find(|s| s.name == name).ok_or_else(|| {
        format!(
            "unknown Jazyk skill `{name}`; one of {}",
            SKILLS.iter().map(|s| s.name).collect::<Vec<_>>().join(", ")
        )
    })?;
    let mut reply = skill.metadata()?;
    reply["body"] = json!(skill.prepared);
    Ok(reply)
}

pub fn tools() -> Vec<Value> {
    let mut tools = tool_payload()["tools"]
        .as_array()
        .expect("embedded tool list")
        .clone();
    tools[1]["inputSchema"]["properties"]["name"]["enum"] =
        json!(SKILLS.iter().map(|s| s.name).collect::<Vec<_>>());
    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_and_exact_bodies_are_self_contained_and_resolve_every_route() {
        let names = SKILLS
            .iter()
            .map(|s| s.name)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names.len(), SKILLS.len());
        for skill in SKILLS {
            let reply = read(skill.name).unwrap();
            assert_eq!(reply["body"], skill.prepared);
            assert!(crate::agent_context::front_matter(skill.body)
                .unwrap()
                .1
                .starts_with("# "));
            assert!(TEMPLATE.contains(&format!("`{}`", skill.name)));
            for token in skill.body.split('`').skip(1).step_by(2) {
                if token.contains('-')
                    && !token.contains(' ')
                    && !token.contains('/')
                    && !token.contains('*')
                {
                    assert!(
                        names.contains(token),
                        "unresolved skill reference `{token}` in {}",
                        skill.name
                    );
                }
            }
        }
        assert!(read("../../secret")
            .unwrap_err()
            .contains("unknown Jazyk skill"));
        assert!(TEMPLATE.contains("{{{sets}}}"));
        assert_eq!(
            catalog().unwrap()["skills"].as_array().unwrap().len(),
            SKILLS.len()
        );
        assert!(TEMPLATE.contains("Never invent requirements"));
        assert!(TEMPLATE.contains("A docs edit does not authorize"));
    }
}
