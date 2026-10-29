//! Pre-standard ACP steering request, with the host-owned idle fallback.
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteeringRequest {
    pub session_id: String,
    pub prompt: Vec<agent_client_protocol::schema::v1::ContentBlock>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}
impl agent_client_protocol::JsonRpcMessage for SteeringRequest {
    fn matches_method(method: &str) -> bool {
        method == "_session/steering"
    }
    fn method(&self) -> &str {
        "_session/steering"
    }
    fn to_untyped_message(
        &self,
    ) -> Result<agent_client_protocol::UntypedMessage, agent_client_protocol::Error> {
        agent_client_protocol::UntypedMessage::new(self.method(), self)
    }
    fn parse_message(
        method: &str,
        params: &impl Serialize,
    ) -> Result<Self, agent_client_protocol::Error> {
        if !Self::matches_method(method) {
            return Err(agent_client_protocol::Error::method_not_found());
        }
        agent_client_protocol::util::json_cast_params(params)
    }
}
impl agent_client_protocol::JsonRpcRequest for SteeringRequest {
    type Response = Value;
}

/// Support is negotiated, with exact upstream versions whose idle contract was
/// verified against the pinned sources in docs/frontends/acp.md.
pub fn supported(handshake: &Value) -> bool {
    let steering = &handshake["_meta"]["steering"];
    if steering["supported"] != true {
        return false;
    }
    steering["idleBehavior"] == "promptRequired"
        || matches!(
            (
                handshake["agentInfo"]["name"].as_str(),
                handshake["agentInfo"]["version"].as_str()
            ),
            (Some("@agentclientprotocol/codex-acp"), Some("2.4.0"))
                | (
                    Some("@agentclientprotocol/claude-agent-acp"),
                    Some("0.99.0")
                )
        )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn support_requires_capability_and_idle_contract_or_exact_pinned_version() {
        for (name, version) in [
            ("@agentclientprotocol/codex-acp", "2.4.0"),
            ("@agentclientprotocol/claude-agent-acp", "0.99.0"),
        ] {
            let mut handshake = serde_json::json!({"agentInfo":{"name":name,"version":version},"_meta":{"steering":{"supported":true}}});
            assert!(supported(&handshake));
            handshake["agentInfo"]["version"] = serde_json::json!("older");
            assert!(!supported(&handshake));
            handshake["_meta"]["steering"]["idleBehavior"] = serde_json::json!("promptRequired");
            assert!(supported(&handshake));
            handshake["_meta"]["steering"]["supported"] = serde_json::json!(false);
            assert!(!supported(&handshake));
        }
    }
}
