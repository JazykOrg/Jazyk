use agent_client_protocol::schema::v1::{Meta, SessionUpdate};
use serde_json::{json, Value};

// The Claude ACP session option and transcript marker. Mirrors
// docs/frontends/acp.md#claude-transcript-forwarding.
pub(super) fn opt_in(meta: &mut Meta) {
    let Some(claude) = meta
        .entry("claudeCode")
        .or_insert_with(|| json!({}))
        .as_object_mut()
    else {
        return;
    };
    let Some(options) = claude
        .entry("options")
        .or_insert_with(|| json!({}))
        .as_object_mut()
    else {
        return;
    };
    options
        .entry("forwardSubagentText")
        .or_insert(Value::Bool(true));
}

pub(super) fn parent_tool_use_id(meta: &Meta) -> Option<&str> {
    meta.get("claudeCode")?
        .get("parentToolUseId")?
        .as_str()
        .filter(|id| !id.trim().is_empty())
}

// The marker links to a tool call on the flattened root transcript. The adapter
// does not supply the embedded agent tree's title, task, depth or write scope.
pub(super) fn tag_transcript(root: &str, update: &mut SessionUpdate) -> bool {
    let meta = match update {
        SessionUpdate::AgentMessageChunk(chunk)
        | SessionUpdate::AgentThoughtChunk(chunk)
        | SessionUpdate::UserMessageChunk(chunk) => &mut chunk.meta,
        SessionUpdate::ToolCall(call) => &mut call.meta,
        SessionUpdate::ToolCallUpdate(call) => &mut call.meta,
        SessionUpdate::Plan(plan) => &mut plan.meta,
        _ => return false,
    };
    let Some(meta) = meta.as_mut() else {
        return false;
    };
    let Some(parent_tool) = parent_tool_use_id(meta) else {
        return false;
    };
    let id = format!("{root}/claude/{parent_tool}");
    if let Some(jazyk) = meta
        .entry("jazyk")
        .or_insert_with(|| json!({}))
        .as_object_mut()
    {
        jazyk
            .entry("agent")
            .or_insert_with(|| json!({"id": id, "parentId": root}));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{NewSessionRequest, SessionNotification};

    #[test]
    fn opt_in_merges_the_claude_session_option_without_overwriting_values() {
        for (input, expected) in [
            (
                json!({}),
                json!({"claudeCode":{"options":{"forwardSubagentText":true}}}),
            ),
            (
                json!({"other":1,"claudeCode":{"extra":"x","options":{"resume":"s"}}}),
                json!({"other":1,"claudeCode":{"extra":"x","options":{"resume":"s","forwardSubagentText":true}}}),
            ),
            (
                json!({"claudeCode":{"options":{"forwardSubagentText":false}}}),
                json!({"claudeCode":{"options":{"forwardSubagentText":false}}}),
            ),
            (
                json!({"claudeCode":{"options":null}}),
                json!({"claudeCode":{"options":null}}),
            ),
            (
                json!({"claudeCode":"opaque"}),
                json!({"claudeCode":"opaque"}),
            ),
        ] {
            let mut req = NewSessionRequest::new("/project");
            req.meta = Some(serde_json::from_value(input).unwrap());
            opt_in(req.meta.as_mut().unwrap());
            assert_eq!(serde_json::to_value(req).unwrap()["_meta"], expected);
        }
    }

    #[test]
    fn claude_transcript_adds_only_the_child_identity_and_the_routing_parent() {
        let rows = [
            json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"child"}}),
            json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"thought"}}),
            json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"child input"}}),
            json!({"sessionUpdate":"tool_call","toolCallId":"child-tool","title":"Read"}),
            json!({"sessionUpdate":"tool_call_update","toolCallId":"child-tool","status":"completed"}),
            json!({"sessionUpdate":"plan","entries":[]}),
        ];
        for mut update in rows {
            update["_meta"] = json!({"claudeCode":{"parentToolUseId":"toolu_a","messageId":"m"},"jazyk":{"trace":1},"other":true});
            let mut n: SessionNotification =
                serde_json::from_value(json!({"sessionId":"root","update":update})).unwrap();
            assert!(tag_transcript("root", &mut n.update));
            let actual = serde_json::to_value(n).unwrap();
            update["_meta"]["jazyk"]["agent"] =
                json!({"id":"root/claude/toolu_a","parentId":"root"});
            assert_eq!(actual, json!({"sessionId":"root","update":update}));
        }
    }

    #[test]
    fn claude_parent_text_and_explicit_agent_metadata_survive() {
        for (meta, tagged) in [
            (json!({"claudeCode":{"messageId":"m"}}), false),
            (json!({"claudeCode":{"parentToolUseId":""}}), false),
            (
                json!({"claudeCode":{"parentToolUseId":"t"},"jazyk":{"agent":{"id":"existing"}}}),
                true,
            ),
        ] {
            let raw = json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"text"},"_meta":meta});
            let mut update = serde_json::from_value(raw.clone()).unwrap();
            assert_eq!(tag_transcript("root", &mut update), tagged);
            assert_eq!(serde_json::to_value(update).unwrap(), raw);
        }
    }
}
