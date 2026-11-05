//! Wire selection and lossless Responses continuation around the public GDK.
use goose_provider_types::{
    conversation::message::Message, formats::openai_responses, model::ModelConfig,
};
use rmcp::model::Tool;
use serde_json::{json, Value};

const NOTE: &str = "jazyk.responses";

pub fn validate(api: &str, summary: Option<&str>) -> Result<(), String> {
    if !matches!(api, "chat_completions" | "responses") {
        return Err("Model API must be chat_completions or responses".into());
    }
    if summary.is_some_and(|s| !matches!(s, "auto" | "concise" | "detailed")) {
        return Err("Reasoning summary must be auto, concise, or detailed".into());
    }
    if api != "responses" && summary.is_some() {
        return Err("Reasoning summary selection requires Responses".into());
    }
    Ok(())
}

/// A bounded adapter connectivity probe, using the same GDK formatter as sessions.
pub fn text_request(model: &str, prompt: &str, max_tokens: u32) -> Result<Value, String> {
    let config = ModelConfig::new(model);
    let messages = [Message::user().with_text(prompt)];
    Ok(
        json!({"model":model,"messages":[{"role":"user","content":prompt}],"max_tokens":max_tokens,
        "_jazykResponsesRequest":companion(&config, "", &messages, &[])?}),
    )
}

pub(crate) fn companion(
    config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
) -> Result<Value, String> {
    let mut body = openai_responses::create_responses_request(config, system, &[], tools)
        .map_err(|e| e.to_string())?;
    let mut segments = Vec::new();
    for message in messages.iter().filter(|m| m.is_agent_visible()) {
        let portable = openai_responses::create_responses_request(
            config,
            "",
            std::slice::from_ref(message),
            &[],
        )
        .map_err(|e| e.to_string())?;
        segments.push(json!({"portable":portable["input"], "native":message.metadata.operation_note(NOTE, "output")}));
    }
    let system_input = body["input"].as_array().cloned().unwrap_or_default();
    let portable: Vec<Value> = system_input
        .iter()
        .cloned()
        .chain(
            segments
                .iter()
                .flat_map(|s| s["portable"].as_array().cloned().unwrap_or_default()),
        )
        .collect();
    let bindings: std::collections::BTreeSet<String> = segments
        .iter()
        .filter_map(|s| s["native"]["binding"].as_str().map(String::from))
        .collect();
    let mut inputs = serde_json::Map::new();
    for binding in bindings {
        let mut input = system_input.clone();
        let mut ids = std::collections::BTreeMap::new();
        for segment in &segments {
            let native = &segment["native"];
            if native["binding"] == binding {
                for (local, original) in native["call_ids"].as_object().into_iter().flatten() {
                    ids.insert(local.clone(), original.clone());
                }
                input.extend(native["items"].as_array().cloned().unwrap_or_default());
            } else {
                input.extend(segment["portable"].as_array().cloned().unwrap_or_default());
            }
        }
        for item in &mut input {
            if item["type"] == "function_call_output" {
                if let Some(id) = item["call_id"].as_str().and_then(|id| ids.get(id)) {
                    item["call_id"] = id.clone();
                }
            }
        }
        inputs.insert(binding, json!(input));
    }
    body["input"] = json!(portable);
    body["_jazykInputs"] = Value::Object(inputs);
    Ok(body)
}

/// Select the provider wire body. Internal metadata never goes to the endpoint.
pub fn request(
    body: &Value,
    api: &str,
    summary: Option<&str>,
    binding: &str,
) -> Result<Value, String> {
    validate(api, summary)?;
    let mut wire = if api == "responses" {
        body.get("_jazykResponsesRequest")
            .cloned()
            .ok_or("The engine does not supply Responses requests")?
    } else {
        body.clone()
    };
    if api == "responses" {
        if let Some(input) = wire["_jazykInputs"].get(binding).cloned() {
            wire["input"] = input;
        }
        wire["model"] = body["model"].clone();
        if let Some(limit) = body.get("max_tokens") {
            wire["max_output_tokens"] = limit.clone();
        }
        wire["store"] = json!(false);
        if let Some(summary) = summary {
            wire["reasoning"]["summary"] = json!(summary);
        } else if let Some(reasoning) = wire.get_mut("reasoning").and_then(Value::as_object_mut) {
            reasoning.remove("summary");
        }
        // Preserve exact call schemas and choices, including non-strict behavior.
        if let Some(choice) = body.get("tool_choice") {
            wire["tool_choice"] = if choice["type"] == "function" {
                json!({"type":"function","name":choice["function"]["name"]})
            } else {
                choice.clone()
            };
        }
        if let Some(format) = body.get("response_format") {
            wire["text"]["format"] = if format["type"] == "json_schema" {
                let mut format = format["json_schema"].clone();
                format["type"] = json!("json_schema");
                format
            } else {
                format.clone()
            };
        }
    }
    wire.as_object_mut()
        .ok_or("Invalid model wire body")?
        .retain(|key, _| !key.starts_with("_jazyk"));
    Ok(wire)
}

/// Return a portable completion for accounting and GDK parsing, retaining opaque output.
pub fn completion(raw: &Value, binding: &str) -> Result<Value, String> {
    if raw["status"] != "completed" {
        return Err(format!(
            "Responses request did not complete: {}",
            raw["status"]
        ));
    }
    let typed: openai_responses::ResponsesApiResponse =
        serde_json::from_value(raw.clone()).map_err(|e| format!("Invalid Responses reply: {e}"))?;
    let message = openai_responses::responses_api_to_message(&typed).map_err(|e| e.to_string())?;
    let formatted = goose_provider_types::formats::openai::format_messages_with_options(
        &[message],
        &goose_provider_types::images::ImageFormat::OpenAi,
        goose_provider_types::formats::openai::OpenAiFormatOptions {
            preserve_thinking_context: true,
            ..Default::default()
        },
    );
    let message = formatted
        .into_iter()
        .next()
        .unwrap_or_else(|| json!({"role":"assistant","content":""}));
    let calls = message["tool_calls"]
        .as_array()
        .is_some_and(|v| !v.is_empty());
    Ok(
        json!({"id":raw["id"],"model":raw["model"],"object":"chat.completion",
        "choices":[{"message":message,"finish_reason":if calls {"tool_calls"} else {"stop"}}],
        "usage":{"prompt_tokens":raw["usage"]["input_tokens"],"completion_tokens":raw["usage"]["output_tokens"],"total_tokens":raw["usage"]["total_tokens"],
        "prompt_tokens_details":{"cached_tokens":raw["usage"]["input_tokens_details"]["cached_tokens"]}},
        "_jazykResponsesOutput":{"binding":binding,"items":raw["output"]}}),
    )
}

pub(crate) fn retain_output(message: &mut Message, value: &Value, ids: Value) {
    if let Some(output) = value.get("_jazykResponsesOutput") {
        let mut output = output.clone();
        output["call_ids"] = ids;
        message.metadata.set_operation_note(NOTE, "output", output);
    }
}

/// SSE framing belongs to the host's HTTP library. Only complete envelopes dispatch tools.
#[derive(Default)]
pub struct ResponseStream {
    completed: Option<Value>,
    failed: bool,
}
impl ResponseStream {
    pub fn event(&mut self, event: &Value) -> Result<Option<(&'static str, String)>, String> {
        match event["type"].as_str() {
            Some("response.output_text.delta") => {
                Ok(event["delta"].as_str().map(|s| ("content", s.into())))
            }
            Some("response.reasoning_summary_text.delta") => Ok(event["delta"]
                .as_str()
                .map(|s| ("reasoning_summary", s.into()))),
            Some("response.reasoning_text.delta") => Ok(event["delta"]
                .as_str()
                .map(|s| ("reasoning_content", s.into()))),
            Some("response.completed") if !self.failed && self.completed.is_none() => {
                self.completed = Some(event["response"].clone());
                Ok(None)
            }
            Some("response.failed" | "response.incomplete" | "error") => {
                self.failed = true;
                Err("Responses stream failed or was incomplete".into())
            }
            Some("response.completed") => Err("Duplicate Responses completion".into()),
            _ => Ok(None),
        }
    }
    pub fn finish(self, binding: &str) -> Result<Value, String> {
        if self.failed {
            return Err("Responses stream failed".into());
        }
        completion(
            &self
                .completed
                .ok_or("Responses stream ended without a completed response")?,
            binding,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_provider_types::conversation::message::MessageContent;
    fn raw() -> Value {
        json!({"id":"resp_1","object":"response","status":"completed","model":"fixture","output":[
        {"type":"reasoning","id":"rs_1","summary":[{"type":"summary_text","text":"Checking records."}],"encrypted_content":"opaque-continuation"},
        {"type":"function_call","id":"fc_1","call_id":"provider-call","name":"read","arguments":"{\"path\":\"a\"}","status":"completed"}],
        "usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110,"input_tokens_details":{"cached_tokens":40}}})
    }
    fn config() -> ModelConfig {
        ModelConfig::new("fixture")
    }
    #[test]
    fn continuation_preserves_opaque_items_and_original_call_ids_only_for_same_route() {
        let raw = raw();
        let value = completion(&raw, "route-a").unwrap();
        let mut message =
            goose_provider_types::formats::openai::response_to_message(&value).unwrap();
        for content in &mut message.content {
            if let MessageContent::ToolRequest(call) = content {
                call.id = "local-call".into();
            }
        }
        retain_output(&mut message, &value, json!({"local-call":"provider-call"}));
        // Exercise serialized recovery, not an in-memory-only metadata path.
        let message: Message =
            serde_json::from_value(serde_json::to_value(message).unwrap()).unwrap();
        let mut tool = Message::user();
        tool.add_tool_response_with_metadata(
            "local-call",
            Ok(rmcp::model::CallToolResult::success(vec![
                rmcp::model::ContentBlock::text("found"),
            ])),
            None,
        );
        let body = json!({"model":"fixture","max_tokens":512,"tool_choice":{"type":"function","function":{"name":"read"}},
            "_jazykResponsesRequest":companion(&config(), "Follow the contract", &[message, tool], &[]).unwrap()});
        let wire = request(&body, "responses", Some("concise"), "route-a").unwrap();
        assert_eq!(wire["input"][1], raw["output"][0]);
        assert_eq!(wire["input"][2], raw["output"][1]);
        assert_eq!(wire["input"][3]["call_id"], "provider-call");
        assert_eq!(wire["reasoning"]["summary"], "concise");
        assert_eq!(
            wire["tool_choice"],
            json!({"type":"function","name":"read"})
        );
        assert_eq!(wire["max_output_tokens"], 512);
        assert_eq!(wire["store"], false);
        assert!(!wire.to_string().contains("_jazyk"));
        let other = request(&body, "responses", None, "route-b").unwrap();
        assert!(!other.to_string().contains("opaque-continuation"));
        assert_eq!(other["input"][2]["call_id"], "local-call");
        assert!(other["reasoning"]["summary"].is_null());
        assert_eq!(value["usage"]["prompt_tokens_details"]["cached_tokens"], 40);
    }
    #[test]
    fn responses_stream_requires_complete_authoritative_envelope() {
        let mut stream = ResponseStream::default();
        assert_eq!(
            stream
                .event(&json!({"type":"response.reasoning_summary_text.delta","delta":"Check"}))
                .unwrap(),
            Some(("reasoning_summary", "Check".into()))
        );
        stream
            .event(
                &json!({"type":"response.function_call_arguments.delta","delta":"{\"partial\":"}),
            )
            .unwrap();
        assert!(stream.finish("route").is_err());
        let mut stream = ResponseStream::default();
        stream
            .event(&json!({"type":"response.completed","response":raw()}))
            .unwrap();
        assert!(stream.finish("route").unwrap()["_jazykResponsesOutput"]["items"].is_array());
        let mut stream = ResponseStream::default();
        assert!(stream
            .event(&json!({"type":"response.incomplete","response":raw()}))
            .is_err());
        assert!(stream.finish("route").is_err());
    }
    #[test]
    fn protocol_selection_never_leaks_internal_fields_and_rejects_invalid_configuration() {
        assert!(request(&json!({}), "responses", None, "route").is_err());
        assert!(validate("guess", None).is_err());
        assert!(validate("chat_completions", Some("auto")).is_err());
        assert!(validate("responses", Some("shorter")).is_err());
        assert_eq!(
            request(
                &json!({"model":"m","_jazykResponsesRequest":{},"_jazykApi":"chat_completions"}),
                "chat_completions",
                None,
                "route"
            )
            .unwrap(),
            json!({"model":"m"})
        );
        let mut incomplete = raw();
        incomplete["status"] = json!("incomplete");
        assert!(completion(&incomplete, "route").is_err());
    }
}
