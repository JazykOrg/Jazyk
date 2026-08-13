// Permission policy for ACP sessions. Worker sessions answer by rule and never block
// on a human; chat sessions forward to the user (the Forward variant arrives with the
// GUI chat pane). Mirrors docs/frontends/acp.md#permissions.
use agent_client_protocol::schema::v1::{
    PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    SelectedPermissionOutcome,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PermissionPolicy {
    // Automated work: allow the agent to proceed. The real safety boundary is the
    // tool serving (validation gates, path sandboxes, leases), not the prompt.
    Auto,
}

pub fn answer(policy: PermissionPolicy, req: &RequestPermissionRequest) -> RequestPermissionOutcome {
    match policy {
        PermissionPolicy::Auto => {
            let allow = req
                .options
                .iter()
                .find(|o| {
                    matches!(
                        o.kind,
                        PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
                    )
                })
                .or_else(|| req.options.first());
            match allow {
                Some(o) => RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                    o.option_id.clone(),
                )),
                None => RequestPermissionOutcome::Cancelled,
            }
        }
    }
}
