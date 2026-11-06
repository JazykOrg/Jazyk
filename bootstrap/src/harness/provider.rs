//! Public GDK formatting with a host-owned transport. No formatter opens files.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use async_trait::async_trait;
use goose_provider_types::{
    base::{stream_from_single_message, MessageStream, Provider},
    conversation::{message::Message, token_usage::ProviderUsage},
    errors::ProviderError,
    formats::openai::{self, OpenAiFormatOptions},
    images::ImageFormat,
    model::ModelConfig,
};
use rmcp::model::Tool;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use super::{LocalFuture, ModelHost, ModelPurpose, ModelRequest, ModelResponse};

pub(crate) enum HostAction {
    Model {
        body: Value,
        purpose: ModelPurpose,
        ordinal: u64,
        reply: oneshot::Sender<Result<(String, Value), String>>,
    },
    Tool {
        id: String,
        name: String,
        args: Value,
        reply: oneshot::Sender<Result<String, String>>,
    },
}

pub(crate) struct ChannelProvider {
    pub tx: mpsc::UnboundedSender<HostAction>,
    pub purpose: ModelPurpose,
    pub ordinal: Arc<AtomicU64>,
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Provider for ChannelProvider {
    fn get_name(&self) -> &str {
        "jazyk"
    }

    async fn stream(
        &self,
        config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let mut body = openai::create_request_with_options(
            config,
            system,
            messages,
            tools,
            &ImageFormat::OpenAi,
            false,
            OpenAiFormatOptions {
                preserve_thinking_context: true,
                supports_vision: false,
                ..Default::default()
            },
        )
        .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;
        body["_jazykResponsesRequest"] = crate::model_api::companion(config, system, messages, tools)
            .map_err(ProviderError::RequestFailed)?;
        let (reply, response) = oneshot::channel();
        self.tx
            .send(HostAction::Model {
                body,
                purpose: self.purpose,
                ordinal: self.ordinal.fetch_add(1, Ordering::Relaxed),
                reply,
            })
            .map_err(|_| ProviderError::RequestFailed("model host closed".into()))?;
        let (attempt_id, mut value) = response
            .await
            .map_err(|_| ProviderError::RequestFailed("model host closed".into()))?
            .map_err(classify_error)?;
        if value["object"] == "response" {
            let binding = value["_jazykResponseRoute"].as_str().unwrap_or_default().to_owned();
            value = crate::model_api::completion(&value, &binding).map_err(ProviderError::RequestFailed)?;
        }
        let (mut message, usage) = if let Some(native) = value.get("_jazykGdkResponse") {
            let message: Message =
                serde_json::from_value(native["message"].clone()).map_err(|e| {
                    ProviderError::RequestFailed(format!("Invalid native GDK response: {e}"))
                })?;
            let usage: ProviderUsage =
                serde_json::from_value(native["usage"].clone()).map_err(|e| {
                    ProviderError::RequestFailed(format!("Invalid native GDK usage: {e}"))
                })?;
            (message, usage)
        } else {
            let message = openai::response_to_message(&value)
                .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;
            let mut usage =
                ProviderUsage::new(config.model_name.clone(), openai::get_usage(&value));
            openai::record_response_metadata(&mut usage, &value);
            (message, usage)
        };
        // Providers can reuse an opaque call id in later replies. GDK tracks
        // answered calls across the conversation, so scope ids to the durable
        // inference identity before they enter that conversation.
        let scope = crate::model::hash_hex(&attempt_id);
        let mut ids = std::collections::HashSet::new();
        let mut original_ids = serde_json::Map::new();
        for content in &mut message.content {
            if let goose_provider_types::conversation::message::MessageContent::ToolRequest(
                request,
            ) = content
            {
                if let Err(error) = &mut request.tool_call {
                    if error.code == rmcp::model::ErrorCode::INVALID_PARAMS {
                        let raw = value["choices"][0]["message"]["tool_calls"].as_array()
                            .and_then(|calls| calls.iter().find(|call| call["id"] == request.id));
                        if let Some(raw) = raw {
                            if let Some(arguments) = raw["function"]["arguments"].as_str() {
                                if let Some(feedback) = super::tool_arguments::json_error_feedback(
                                    raw["function"]["name"].as_str().unwrap_or("unknown"),
                                    arguments, value["choices"][0]["finish_reason"].as_str(),
                                ) {
                                    error.message = feedback.into();
                                }
                            }
                        }
                    }
                }
                if !ids.insert(request.id.clone()) {
                    return Err(ProviderError::RequestFailed("Model returned a duplicate tool-call id in one response; no tools executed".into()));
                }
                let original = request.id.clone();
                request.id = format!("{}:{}", &scope[..16], request.id);
                original_ids.insert(request.id.clone(), serde_json::json!(original));
            }
        }
        crate::model_api::retain_output(&mut message, &value, Value::Object(original_ids));
        Ok(stream_from_single_message(message, usage))
    }
}

fn classify_error(error: String) -> ProviderError {
    let lower = error.to_ascii_lowercase();
    if lower.contains("context length") || lower.contains("context_length_exceeded") {
        ProviderError::ContextLengthExceeded(error)
    } else if lower.contains("tools-rejected")
        || lower.contains("does not support tools")
        || lower.contains("tool calling is not supported")
    {
        ProviderError::RequestFailed(format!(
            "Unsupported model capability: native tool calling is required. {error}"
        ))
    } else {
        ProviderError::RequestFailed(error)
    }
}

pub struct DefaultModelHost {
    llm: crate::llm::Llm,
}

impl DefaultModelHost {
    pub fn new(llm: &crate::llm::Llm) -> Self {
        Self { llm: llm.clone() }
    }
}

impl ModelHost for DefaultModelHost {
    fn infer<'a>(
        &'a mut self,
        request: ModelRequest,
    ) -> LocalFuture<'a, Result<ModelResponse, String>> {
        Box::pin(async move {
            let body = self.llm.request_async(request).await?;
            Ok(ModelResponse { body })
        })
    }
}
