//! Bounded evidence for old protected Stops that saved the visible timeline alone.
use goose_provider_types::conversation::message::Message;
use serde_json::{json, Value};

const NOTE: &str = "jazyk.chat-history";
const RECEIPT: &str = include_str!("../../../docs/compiler/goals/prompts/chat-history-recovery.md");
const TURN_BYTES: usize = 6 * 1024;

fn bounded(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_owned();
    }
    let first: String = text.chars().take(chars / 2).collect();
    let last: String = text
        .chars()
        .rev()
        .take(chars / 2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{first}\n[truncated]\n{last}")
}

fn receipt_turns(updates: &[(u64, Value)]) -> Vec<Value> {
    let mut turns = Vec::<Value>::new();
    for (n, update) in updates {
        let kind = update["sessionUpdate"].as_str().unwrap_or_default();
        if kind == "user_message" {
            let text = update["text"].as_str().unwrap_or_default();
            turns.push(json!({"n":n,"user":bounded(text,2048),
                "assistant":"","observedToolUpdates":[],"truncated":text.chars().count()>2048}));
            if turns.len() > 2 {
                turns.remove(0);
                turns[0]["truncated"] = json!(true);
            }
            continue;
        }
        let Some(turn) = turns.last_mut() else {
            continue;
        };
        match kind {
            "agent_message" | "agent_message_chunk" => {
                let text = if kind == "agent_message" {
                    &update["text"]
                } else {
                    &update["content"]["text"]
                };
                let combined = format!(
                    "{}{}",
                    turn["assistant"].as_str().unwrap_or_default(),
                    text.as_str().unwrap_or_default()
                );
                if combined.chars().count() > 2048 {
                    turn["truncated"] = json!(true);
                }
                turn["assistant"] = json!(bounded(&combined, 2048));
            }
            "tool_call" | "tool_call_update" => {
                let tools = turn["observedToolUpdates"].as_array_mut().unwrap();
                let id = &update["toolCallId"];
                let position = tools.iter().position(|tool| tool["id"] == *id);
                let index = position.unwrap_or_else(|| {
                    tools.push(json!({"id":id}));
                    tools.len() - 1
                });
                let record = &mut tools[index];
                if kind == "tool_call" {
                    record["tool"] = update["title"].clone();
                    if let Some(input) = update.get("rawInput") {
                        record["input"] = json!(bounded(&input.to_string(), 256));
                    }
                } else {
                    record["status"] = update["status"].clone();
                    if let Some(output) = update.get("rawOutput") {
                        record["output"] = json!(bounded(&output.to_string(), 512));
                    }
                }
                if tools.len() > 8 {
                    tools.remove(0);
                    turn["truncated"] = json!(true);
                }
            }
            "turn_end" => {
                turn["stop"] = json!(bounded(update["stop"].as_str().unwrap_or_default(), 64))
            }
            _ => {}
        }
    }
    for turn in &mut turns {
        while serde_json::to_vec(turn).map_or(usize::MAX, |bytes| bytes.len()) > TURN_BYTES {
            turn["truncated"] = json!(true);
            let tools = turn["observedToolUpdates"].as_array_mut().unwrap();
            if !tools.is_empty() {
                tools.remove(0);
                continue;
            }
            let field = if turn["assistant"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .count()
                > 128
            {
                "assistant"
            } else {
                "user"
            };
            let text = turn[field].as_str().unwrap_or_default();
            let smaller = text.chars().count() / 2;
            if smaller < 64 {
                break;
            }
            turn[field] = json!(bounded(text, smaller));
        }
    }
    turns
}

pub(super) fn is_receipt(message: &Value) -> bool {
    message["_jazykGdk"]["metadata"]["operations"][NOTE]["through"].is_u64()
}

/// Match every retained user message against the exact suffix of saved human text.
/// Generated compaction continuation messages cannot match and are left alone.
fn retained_start(history: &[Value], humans: &[(u64, &str)]) -> Option<usize> {
    let mut cursor = humans.len();
    let mut found = false;
    for message in history
        .iter()
        .rev()
        .filter(|message| message["role"] == "user" && !is_receipt(message))
    {
        found = true;
        let content = message["content"].as_str()?;
        let mut start = cursor;
        let mut remaining = content;
        let mut matched = None;
        while start > 0 {
            start -= 1;
            let prefix = remaining.strip_suffix(humans[start].1)?;
            if prefix.is_empty() {
                matched = Some(start);
                break;
            }
            remaining = prefix.strip_suffix("\n\n")?;
        }
        cursor = matched?;
    }
    (found || history.is_empty()).then_some(cursor)
}

pub(super) fn restore(history: &mut Vec<Value>, updates: &[(u64, Value)], current: &str) -> bool {
    if updates.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
        return false;
    }
    let Some(before) = updates.iter().rposition(|(_, update)| {
        update["sessionUpdate"] == "user_message" && update["messageId"] == current
    }) else {
        return false;
    };
    let prior = &updates[..before];
    let humans = prior
        .iter()
        .filter_map(|(n, update)| {
            (update["sessionUpdate"] == "user_message")
                .then(|| update["text"].as_str().map(|text| (*n, text)))
                .flatten()
        })
        .collect::<Vec<_>>();
    let Some(start) = retained_start(history, &humans) else {
        return false;
    };
    if start == 0 {
        return false;
    }
    let end = humans
        .get(start)
        .map(|(n, _)| *n)
        .unwrap_or_else(|| prior.last().map_or(0, |(n, _)| n.saturating_add(1)));
    let missing_end = prior.partition_point(|(n, _)| *n < end);
    let missing = &prior[..missing_end];
    let terminal = missing
        .iter()
        .rev()
        .find(|(_, update)| update["sessionUpdate"] == "turn_end");
    if !terminal.is_some_and(|(_, update)| {
        matches!(update["stop"].as_str(), Some("cancelled" | "interrupted"))
    }) {
        return false;
    }
    let Some(through) = missing.last().map(|(n, _)| *n) else {
        return false;
    };
    let covered = history
        .iter()
        .filter_map(|value| value.get("_jazykGdk"))
        .filter_map(|value| serde_json::from_value::<Message>(value.clone()).ok())
        .filter_map(|message| {
            message
                .metadata
                .operation_note(NOTE, "through")
                .and_then(Value::as_u64)
        })
        .max();
    if covered.is_some_and(|covered| covered >= through) {
        return false;
    }
    let begin = covered.map_or(0, |covered| missing.partition_point(|(n, _)| *n <= covered));
    let turns = receipt_turns(&missing[begin..]);
    if turns.is_empty() {
        return false;
    }
    let text = RECEIPT.replace(
        "{HISTORY}",
        &json!({"through":through,"turns":turns}).to_string(),
    );
    let mut native = Message::user().with_text(&text);
    native.metadata.user_visible = false;
    native
        .metadata
        .set_operation_note(NOTE, "through", json!(through));
    history.insert(0, json!({"role":"user","content":text,"_jazykGdk":native}));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str, text: &str) -> Value {
        json!({"sessionUpdate":"user_message","messageId":id,"text":text})
    }

    fn legacy() -> Vec<(u64, Value)> {
        vec![
            (
                0,
                user(
                    "original",
                    "Restyle the UML diagram in deliverable/index.html",
                ),
            ),
            (
                1,
                json!({"sessionUpdate":"agent_message_chunk","content":{"text":"Updating UML cards."}}),
            ),
            (
                2,
                json!({"sessionUpdate":"tool_call","toolCallId":"write","title":"write_text_file","rawInput":{"path":"deliverable/index.html"}}),
            ),
            (
                3,
                json!({"sessionUpdate":"tool_call_update","toolCallId":"write","status":"completed","rawOutput":"Saved deliverable/index.html"}),
            ),
            (4, json!({"sessionUpdate":"turn_end","stop":"cancelled"})),
            (5, user("retry", "try again")),
            (
                6,
                json!({"sessionUpdate":"agent_message_chunk","content":{"text":"I do not have the earlier request."}}),
            ),
            (7, json!({"sessionUpdate":"turn_end","stop":"end_turn"})),
            (8, user("current", "continue")),
        ]
    }

    fn amnesic() -> Vec<Value> {
        vec![
            json!({"role":"user","content":"try again"}),
            json!({"role":"assistant","content":"I do not have the earlier request."}),
        ]
    }

    #[test]
    fn repairs_an_already_amnesic_chat_once_without_inventing_tool_messages() {
        let mut history = amnesic();
        assert!(restore(&mut history, &legacy(), "current"));
        let receipt = history[0]["content"].as_str().unwrap();
        assert!(receipt.contains("Restyle the UML diagram"));
        assert!(receipt.contains("Updating UML cards."));
        assert!(receipt.contains("Saved deliverable/index.html"));
        assert!(!history
            .iter()
            .any(|message| message["role"] == "tool" || message.get("tool_calls").is_some()));
        let before = history.clone();
        assert!(!restore(&mut history, &legacy(), "current"));
        assert_eq!(history, before);
    }

    #[test]
    fn healthy_and_compacted_histories_are_not_expanded() {
        let updates = legacy();
        let mut healthy = vec![
            json!({"role":"user","content":"Restyle the UML diagram in deliverable/index.html"}),
            json!({"role":"assistant","content":"Updating UML cards."}),
            json!({"role":"user","content":"try again"}),
        ];
        let before = healthy.clone();
        assert!(!restore(&mut healthy, &updates, "current"));
        assert_eq!(healthy, before);
        let mut compacted = vec![
            json!({"role":"assistant","content":"Summary of earlier work"}),
            json!({"role":"user","content":"Continue the active session `chat chat-1` using the preserved context."}),
            json!({"role":"user","content":"try again"}),
        ];
        let before = compacted.clone();
        assert!(!restore(&mut compacted, &updates, "current"));
        assert_eq!(compacted, before);
    }

    #[test]
    fn a_receipt_does_not_hide_a_later_missing_prefix_or_confuse_repeated_retry_text() {
        let mut history = amnesic();
        let mut updates = legacy();
        assert!(restore(&mut history, &updates, "current"));
        let prior_receipt = history[0].clone();
        updates.extend([
            (9,json!({"sessionUpdate":"turn_end","stop":"cancelled"})),
            (10,user("retry-again","try again")),
            (11,json!({"sessionUpdate":"agent_message_chunk","content":{"text":"Which earlier work?"}})),
            (12,json!({"sessionUpdate":"turn_end","stop":"end_turn"})),
            (13,user("next","continue")),
        ]);
        let mut corrupted = vec![
            prior_receipt,
            json!({"role":"user","content":"try again"}),
            json!({"role":"assistant","content":"Which earlier work?"}),
        ];
        assert!(restore(&mut corrupted, &updates, "next"));
        let native: Message = serde_json::from_value(corrupted[0]["_jazykGdk"].clone()).unwrap();
        assert_eq!(
            native.metadata.operation_note(NOTE, "through"),
            Some(&json!(9))
        );
        assert!(!restore(&mut corrupted, &updates, "next"));
    }

    #[test]
    fn merged_human_messages_and_an_empty_stopped_history_can_be_repaired() {
        let mut empty = Vec::new();
        assert!(restore(&mut empty, &legacy(), "retry"));
        let mut updates = legacy();
        updates.insert(8, (8, user("another", "more detail")));
        updates[9].0 = 9;
        let mut history = vec![json!({"role":"user","content":"try again\n\nmore detail"})];
        assert!(restore(&mut history, &updates, "current"));
    }

    #[test]
    fn missing_uninterrupted_history_or_an_unordered_timeline_is_left_alone() {
        let mut updates = legacy();
        updates[4].1["stop"] = json!("end_turn");
        assert!(!restore(&mut amnesic(), &updates, "current"));
        let mut updates = legacy();
        updates.swap(1, 2);
        assert!(!restore(&mut amnesic(), &updates, "current"));
    }

    #[test]
    fn receipts_are_bounded_and_missing_tool_payloads_stay_missing() {
        let mut updates = Vec::new();
        let mut n = 0;
        for turn in 0..4 {
            updates.push((
                n,
                user(&format!("old-{turn}"), &"\u{1f916}\n".repeat(20_000)),
            ));
            n += 1;
            updates.push((n,json!({"sessionUpdate":"agent_message_chunk","content":{"text":"large reply ".repeat(20_000)}})));
            n += 1;
            for tool in 0..12 {
                updates.push((n,json!({"sessionUpdate":"tool_call","toolCallId":format!("{turn}-{tool}"),"title":"write_text_file"})));
                n += 1;
                updates.push((n,json!({"sessionUpdate":"tool_call_update","toolCallId":format!("{turn}-{tool}"),"status":"completed"})));
                n += 1;
            }
            updates.push((n, json!({"sessionUpdate":"turn_end","stop":"cancelled"})));
            n += 1;
        }
        updates.push((n, user("current", "try again")));
        let mut history = Vec::new();
        assert!(restore(&mut history, &updates, "current"));
        let content = history[0]["content"].as_str().unwrap();
        assert!(content.len() < 16 * 1024, "{} bytes", content.len());
        assert!(content.contains("truncated"));
        assert!(!content.contains("\"output\""));
    }
}
