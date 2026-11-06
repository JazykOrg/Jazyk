//! Exact tool evidence retained outside the model's progress summary.
use std::collections::BTreeSet;

use goose_provider_types::conversation::message::Message;
use rmcp::model::Role;

pub(crate) struct Context {
    pub opening: Option<Message>,
    pub summary: Vec<Message>,
    pub retained: Vec<Message>,
}

pub(crate) fn context(messages: &[Message], conversation: bool) -> Context {
    let opening = (!conversation)
        .then(|| {
            messages
                .first()
                .filter(|message| message.role == Role::User)
                .cloned()
        })
        .flatten();
    let start = usize::from(opening.is_some());
    let mut keep = BTreeSet::new();
    if let Some(response) = messages
        .iter()
        .rposition(|message| !message.get_tool_response_ids().is_empty())
    {
        if let Some(request) = messages[..response].iter().rposition(|message| {
            message
                .content
                .iter()
                .any(|content| content.as_tool_request().is_some())
        }) {
            keep.extend(request..messages.len());
        }
    }
    // One exact package per unfinished target. A successful record closes its
    // package; a later successful begin opens a new snapshot of that target.
    let succeeded: BTreeSet<_> = messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|content| content.as_tool_response())
        .filter(|response| {
            response
                .tool_result
                .as_ref()
                .is_ok_and(|result| result.is_error != Some(true))
        })
        .map(|response| response.id.as_str())
        .collect();
    let mut seen = BTreeSet::new();
    for (index, message) in messages.iter().enumerate().rev() {
        let mut preserve = false;
        for request in message
            .content
            .iter()
            .rev()
            .filter_map(|content| content.as_tool_request())
        {
            if !succeeded.contains(request.id.as_str()) {
                continue;
            }
            let Ok(call) = &request.tool_call else {
                continue;
            };
            let (kind, field, begin) = match call.name.as_ref() {
                "begin_generation" => ("generate", "entity", true),
                "record_generation" => ("generate", "entity", false),
                "begin_binding" => ("bind", "requirement", true),
                "record_binding" => ("bind", "requirement", false),
                "begin_verification" => ("verify", "requirement", true),
                "record_verdict" => ("verify", "requirement", false),
                _ => continue,
            };
            let Some(target) = call
                .arguments
                .as_ref()
                .and_then(|args| args.get(field))
                .and_then(|value| value.as_str())
            else {
                continue;
            };
            if seen.insert((kind, target)) && begin {
                preserve = true;
            }
        }
        if !preserve {
            continue;
        }
        // Preserve the whole request batch so every result stays paired.
        let batch_ids: BTreeSet<_> = message
            .content
            .iter()
            .filter_map(|content| content.as_tool_request())
            .map(|request| request.id.as_str())
            .collect();
        keep.insert(index);
        for (offset, response) in messages[index + 1..].iter().enumerate() {
            if response
                .get_tool_response_ids()
                .iter()
                .any(|id| batch_ids.contains(id))
            {
                keep.insert(index + 1 + offset);
            }
        }
    }
    let mut summary = Vec::new();
    let mut retained = Vec::new();
    for (index, message) in messages.iter().enumerate().skip(start) {
        if keep.contains(&index) {
            let mut exact = message.clone();
            if exact
                .content
                .iter()
                .any(|content| content.as_tool_request().is_some())
            {
                let mut reasoning = exact.clone();
                reasoning
                    .content
                    .retain(|content| content.as_tool_request().is_none());
                if !reasoning.content.is_empty() {
                    summary.push(reasoning);
                }
                exact
                    .content
                    .retain(|content| content.as_tool_request().is_some());
            }
            retained.push(exact);
        } else {
            summary.push(message.clone());
        }
    }
    Context {
        opening,
        summary,
        retained,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
    fn request(id: &str, name: &str) -> Message {
        Message::assistant().with_tool_request(
            id,
            Ok(CallToolRequestParams::new(name.to_owned()).with_arguments(
                serde_json::from_value(
                    serde_json::json!({"entity":"ent:target","requirement":"req:target"}),
                )
                .unwrap(),
            )),
        )
    }
    fn reply(id: &str, text: &str, error: bool) -> Message {
        let mut message = Message::user();
        let result = if error {
            CallToolResult::error(vec![ContentBlock::text(text.to_owned())])
        } else {
            CallToolResult::success(vec![ContentBlock::text(text.to_owned())])
        };
        message.add_tool_response_with_metadata(id, Ok(result), None);
        message
    }
    #[test]
    fn package_survives_later_reads_writes_and_failed_package_reload() {
        let messages = vec![
            Message::user().with_text("Immutable batch"),
            request("package", "begin_generation"),
            reply("package", "exact classname::selector and factHash", false),
            request("read", "read_text_file"),
            reply("read", "old source", false),
            request("write", "write_text_file"),
            reply("write", "source updated", false),
            request("refused", "begin_generation"),
            reply("refused", "not ready", true),
            Message::user().with_text("Keep my requested layout"),
        ];
        let parts = context(&messages, false);
        assert_eq!(
            parts.opening.as_ref().unwrap().as_concat_text(),
            "Immutable batch"
        );
        let ids: BTreeSet<_> = parts
            .retained
            .iter()
            .flat_map(|message| message.get_tool_response_ids())
            .collect();
        assert!(ids.contains("package") && ids.contains("refused"));
        assert!(!ids.contains("read") && !ids.contains("write"));
        assert_eq!(
            parts.retained.last().unwrap().as_concat_text(),
            "Keep my requested layout"
        );
        assert!(!parts
            .summary
            .iter()
            .any(|message| message.get_tool_response_ids().contains("package")));
    }
    #[test]
    fn newest_successful_package_replaces_old_and_batched_results_stay_paired() {
        let mut batch = request("fresh", "begin_generation");
        batch
            .content
            .extend(request("side", "task_context").content);
        let messages = vec![
            Message::user().with_text("Batch"),
            request("old", "begin_generation"),
            reply("old", "old hash", false),
            batch,
            reply("fresh", "current exact bindings", false),
            reply("side", "tasks", false),
            request("write", "write_text_file"),
            reply("write", "changed", false),
        ];
        let parts = context(&messages, false);
        let ids: BTreeSet<_> = parts
            .retained
            .iter()
            .flat_map(|message| message.get_tool_response_ids())
            .collect();
        assert_eq!(ids, BTreeSet::from(["fresh", "side", "write"]));
        let requests: BTreeSet<_> = parts
            .retained
            .iter()
            .flat_map(|message| &message.content)
            .filter_map(|content| content.as_tool_request())
            .map(|request| request.id.as_str())
            .collect();
        assert_eq!(ids, requests);
    }
    #[test]
    fn records_release_packages_and_new_active_targets_keep_exact_snapshots() {
        let mut target_b = request("b", "begin_generation");
        target_b
            .content
            .iter_mut()
            .find_map(|content| match content {
                goose_provider_types::conversation::message::MessageContent::ToolRequest(
                    request,
                ) => Some(request),
                _ => None,
            })
            .unwrap()
            .tool_call
            .as_mut()
            .unwrap()
            .arguments
            .as_mut()
            .unwrap()
            .insert("entity".into(), serde_json::json!("ent:b"));
        let messages = vec![
            Message::user().with_text("Batch"),
            request("a-old", "begin_generation"),
            reply("a-old", "old A hash", false),
            request("a-record", "record_generation"),
            reply("a-record", "A recorded", false),
            target_b,
            reply("b", "B exact hash and bound tests", false),
            request("a-new", "begin_generation"),
            reply("a-new", "new A snapshot", false),
            request("read", "read_text_file"),
            reply("read", "source", false),
        ];
        let parts = context(&messages, false);
        let ids: BTreeSet<_> = parts
            .retained
            .iter()
            .flat_map(|message| message.get_tool_response_ids())
            .collect();
        assert_eq!(ids, BTreeSet::from(["b", "a-new", "read"]));
        assert_eq!(
            parts
                .retained
                .iter()
                .filter(|message| message.get_tool_response_ids().contains("a-new"))
                .count(),
            1
        );
        let closed = context(&messages[..5], false);
        assert!(!closed
            .retained
            .iter()
            .any(|message| message.get_tool_response_ids().contains("a-old")));
    }
}
