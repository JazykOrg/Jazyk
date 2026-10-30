#![cfg(target_arch = "wasm32")]
#![allow(clippy::arc_with_non_send_sync)]

use std::{cell::RefCell, collections::VecDeque, rc::Rc, sync::Arc};

use async_trait::async_trait;
use goose_context_management::{
    compact, summarize, CompactingProvider, CompactionModel, CompactionOutput, ProviderModel,
    Templates, TokenEstimator,
};
use goose_provider_types::{
    base::{collect_stream, stream_from_single_message, MessageStream, Provider},
    conversation::{
        message::Message,
        token_usage::{ProviderUsage, Usage},
    },
    errors::ProviderError,
    model::ModelConfig,
};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;

async fn host_call() {
    JsFuture::from(js_sys::Promise::resolve(&js_sys::Object::new()))
        .await
        .unwrap();
}

struct HostModel {
    requests: Rc<RefCell<Vec<String>>>,
}

#[async_trait(?Send)]
impl CompactionModel for HostModel {
    async fn complete(
        &self,
        system: &str,
        _messages: &[Message],
    ) -> Result<(Message, ProviderUsage), ProviderError> {
        host_call().await;
        self.requests.borrow_mut().push(system.to_owned());
        Ok((Message::assistant().with_text("retained context"), usage()))
    }
}

struct HostEstimator {
    calls: Rc<RefCell<usize>>,
}

#[async_trait(?Send)]
impl TokenEstimator for HostEstimator {
    async fn count_chat_tokens(&self, _system: &str, _messages: &[Message]) -> usize {
        host_call().await;
        *self.calls.borrow_mut() += 1;
        41
    }

    async fn count_text_tokens(&self, _text: &str) -> usize {
        host_call().await;
        *self.calls.borrow_mut() += 1;
        7
    }
}

#[derive(Default)]
struct HostOutput {
    summary: Rc<RefCell<Option<Message>>>,
    usage: Rc<RefCell<Option<ProviderUsage>>>,
}

impl CompactionOutput for HostOutput {
    fn set_summary(&mut self, summary: Message) {
        *self.summary.borrow_mut() = Some(summary);
    }

    fn set_usage(&mut self, usage: ProviderUsage) {
        *self.usage.borrow_mut() = Some(usage);
    }
}

fn usage() -> ProviderUsage {
    ProviderUsage::new("host-model".to_owned(), Usage::default())
}

#[wasm_bindgen_test]
async fn compact_accepts_promise_backed_model_estimator_and_output() {
    let requests = Rc::new(RefCell::new(Vec::new()));
    let estimator_calls = Rc::new(RefCell::new(0));
    let model = HostModel {
        requests: requests.clone(),
    };
    let estimator = HostEstimator {
        calls: estimator_calls.clone(),
    };
    let messages = vec![Message::user().with_text("preserve this requirement")];
    let mut output = HostOutput::default();

    compact(&model, Some(&estimator), &messages, &mut output)
        .await
        .unwrap();

    assert_eq!(requests.borrow().len(), 1);
    assert!(requests.borrow()[0].contains("preserve this requirement"));
    assert_eq!(*estimator_calls.borrow(), 2);
    let summary = output.summary.borrow();
    let summary = summary.as_ref().unwrap();
    assert_eq!(summary.as_concat_text(), "retained context");
    assert_eq!(summary.role, rmcp::model::Role::User);
    let usage = output.usage.borrow();
    let usage = &usage.as_ref().unwrap().usage;
    assert_eq!(usage.input_tokens, Some(41));
    assert_eq!(usage.output_tokens, Some(7));
    assert_eq!(usage.total_tokens, Some(48));
}

type Reply = Result<(Message, ProviderUsage), ProviderError>;

struct HostProvider {
    replies: Rc<RefCell<VecDeque<Reply>>>,
    requests: Rc<RefCell<Vec<Vec<Message>>>>,
}

impl HostProvider {
    fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        Self {
            replies: Rc::new(RefCell::new(replies.into_iter().collect())),
            requests: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

#[async_trait(?Send)]
impl Provider for HostProvider {
    fn get_name(&self) -> &str {
        "host"
    }

    async fn stream(
        &self,
        _model_config: &ModelConfig,
        _system: &str,
        messages: &[Message],
        _tools: &[rmcp::model::Tool],
    ) -> Result<MessageStream, ProviderError> {
        host_call().await;
        self.requests.borrow_mut().push(messages.to_vec());
        let (message, usage) = self.replies.borrow_mut().pop_front().unwrap()?;
        Ok(stream_from_single_message(message, usage))
    }
}

#[wasm_bindgen_test]
async fn provider_model_accepts_a_promise_backed_provider() {
    let provider = Arc::new(HostProvider::new([Ok((
        Message::assistant().with_text("provider summary"),
        usage(),
    ))]));
    let model = ProviderModel::new(provider.clone(), ModelConfig::new("host-model"));

    let summary = summarize(
        &model,
        None,
        &Templates::default(),
        &[Message::user().with_text("source conversation")],
    )
    .await
    .unwrap();

    assert_eq!(summary.message.as_concat_text(), "provider summary");
    assert_eq!(summary.usage.model, "host-model");
    assert_eq!(provider.requests.borrow().len(), 1);
}

#[wasm_bindgen_test]
async fn compacting_provider_retries_with_summary_after_context_overflow() {
    let provider = Arc::new(HostProvider::new([
        Err(ProviderError::ContextLengthExceeded("oversized".to_owned())),
        Ok((Message::assistant().with_text("compacted context"), usage())),
        Ok((Message::assistant().with_text("continued answer"), usage())),
    ]));
    let compacting = CompactingProvider::new(provider.clone());
    let stream = compacting
        .stream(
            &ModelConfig::new("host-model"),
            "system",
            &[Message::user().with_text("original conversation")],
            &[],
        )
        .await
        .unwrap();
    let (answer, _) = collect_stream(stream).await.unwrap();

    assert_eq!(answer.as_concat_text(), "continued answer");
    let requests = provider.requests.borrow();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0][0].as_concat_text(), "original conversation");
    assert_eq!(requests[2][0].as_concat_text(), "compacted context");
    assert_eq!(requests[2][0].role, rmcp::model::Role::User);
    assert!(provider.replies.borrow().is_empty());
}
