// The ACP bridge: jazyk as a client of one downstream ACP agent (worker and chat
// sessions), and as an agent/proxy toward IDEs. Mirrors docs/frontends/acp.md.
pub mod agent;
pub mod commands;
pub mod config;
pub mod host;
pub mod http;
pub mod install;
pub mod plan;
pub mod policy;
pub mod proxy;
pub mod runner;
pub mod sessions;
pub mod translate;

pub mod steering;

// A typed tool call update decorated from the effects its output carries: standard
// diff content, locations, and `_meta.jazyk.effects`. Any other update passes as it
// is. Mirrors docs/frontends/acp.md#edit-effects.
pub fn decorate_effects(update: &mut agent_client_protocol::schema::v1::SessionUpdate) {
    use agent_client_protocol::schema::v1::SessionUpdate;
    if !matches!(update, SessionUpdate::ToolCallUpdate(_)) {
        return;
    }
    let Ok(mut v) = serde_json::to_value(&*update) else {
        return;
    };
    if !v["_meta"]["jazyk"]["effects"].is_null() {
        return;
    }
    crate::effects::decorate(&mut v);
    if v["_meta"]["jazyk"]["effects"].is_null() {
        return;
    }
    if let Ok(decorated) = serde_json::from_value(v) {
        *update = decorated;
    }
}
