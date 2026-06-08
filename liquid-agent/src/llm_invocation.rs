use std::{collections::BTreeMap, future::Future, sync::Arc};

use anyhow::Result;
use futures_util::StreamExt;
use liquid_llm::{LlmClient, LlmEvent, LlmRequest, LlmResponse, ToolCall};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LlmInvocationMode {
    Complete,
    StreamWithFallback,
}

impl LlmInvocationMode {
    pub(crate) fn from_streaming_enabled(streaming_enabled: bool) -> Self {
        if streaming_enabled {
            Self::StreamWithFallback
        } else {
            Self::Complete
        }
    }
}

pub(crate) async fn invoke_llm(
    llm: &Arc<dyn LlmClient>,
    request: LlmRequest,
    mode: LlmInvocationMode,
) -> Result<LlmResponse> {
    invoke_llm_with_text_delta(llm, request, mode, |_| async {}).await
}

pub(crate) async fn invoke_llm_with_text_delta<F, Fut>(
    llm: &Arc<dyn LlmClient>,
    request: LlmRequest,
    mode: LlmInvocationMode,
    mut on_text_delta: F,
) -> Result<LlmResponse>
where
    F: FnMut(String) -> Fut + Send,
    Fut: Future<Output = ()> + Send,
{
    match mode {
        LlmInvocationMode::Complete => llm.complete(request).await,
        LlmInvocationMode::StreamWithFallback => {
            let fallback_request = request.clone();
            match collect_streamed_response(llm, request, &mut on_text_delta).await {
                Ok(response) => Ok(response),
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "LLM streaming request failed; falling back to non-streaming completion"
                    );
                    llm.complete(fallback_request).await
                }
            }
        }
    }
}

async fn collect_streamed_response<F, Fut>(
    llm: &Arc<dyn LlmClient>,
    request: LlmRequest,
    on_text_delta: &mut F,
) -> Result<LlmResponse>
where
    F: FnMut(String) -> Fut + Send,
    Fut: Future<Output = ()> + Send,
{
    let mut stream = llm.stream(request).await?;
    let mut content = String::new();
    let mut tool_call_builders = BTreeMap::<usize, ToolCallBuilder>::new();
    let mut tool_calls = Vec::new();
    let mut output_items = Vec::new();
    let mut raw_events = Vec::new();
    let mut done_response = None;

    while let Some(event) = stream.next().await {
        match event? {
            LlmEvent::TextDelta(delta) => {
                on_text_delta(delta.clone()).await;
                content.push_str(&delta);
            }
            LlmEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => {
                let key = index.unwrap_or(tool_call_builders.len());
                tool_call_builders
                    .entry(key)
                    .or_default()
                    .push(id, name, &arguments_delta);
            }
            LlmEvent::ToolCall(tool_call) => {
                tool_calls.push(tool_call);
            }
            LlmEvent::MessageDone(response) => {
                done_response = Some(response);
            }
            LlmEvent::RawJson(raw) => {
                raw_events.push(raw);
            }
            LlmEvent::Done => break,
        }
    }

    if let Some(response) = &done_response {
        if content.is_empty() {
            content = response.content.clone();
        }
        if tool_calls.is_empty() {
            tool_calls = response.tool_calls.clone();
        }
        output_items = response.output_items.clone();
    }

    tool_calls.extend(
        tool_call_builders
            .into_iter()
            .map(|(index, builder)| builder.finish(index)),
    );

    if content.is_empty() && tool_calls.is_empty() && output_items.is_empty() {
        anyhow::bail!("LLM stream returned no usable events");
    }

    Ok(LlmResponse {
        id: done_response.and_then(|response| response.id),
        content,
        tool_calls,
        output_items,
        raw: serde_json::json!({ "stream_events": raw_events }),
    })
}

#[derive(Debug, Default)]
struct ToolCallBuilder {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl ToolCallBuilder {
    fn push(&mut self, id: Option<String>, name: Option<String>, arguments_delta: &str) {
        if id.is_some() {
            self.id = id;
        }
        if name.is_some() {
            self.name = name;
        }
        self.arguments.push_str(arguments_delta);
    }

    fn finish(self, index: usize) -> ToolCall {
        ToolCall::new(
            self.id.unwrap_or_else(|| format!("tool_call_{index}")),
            self.name.unwrap_or_default(),
            if self.arguments.is_empty() {
                "{}".to_owned()
            } else {
                self.arguments
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use anyhow::{Result, anyhow};
    use async_trait::async_trait;
    use futures_util::stream;
    use liquid_llm::{LlmMessage, LlmProtocol};

    use super::*;

    struct StreamingTestClient {
        events: Mutex<VecDeque<LlmEvent>>,
        fallback: LlmResponse,
        fail_stream: bool,
        stream_calls: Mutex<usize>,
        complete_calls: Mutex<usize>,
    }

    impl StreamingTestClient {
        fn new(events: Vec<LlmEvent>, fallback: LlmResponse) -> Self {
            Self {
                events: Mutex::new(events.into()),
                fallback,
                fail_stream: false,
                stream_calls: Mutex::new(0),
                complete_calls: Mutex::new(0),
            }
        }

        fn failing_stream(fallback: LlmResponse) -> Self {
            Self {
                events: Mutex::new(VecDeque::new()),
                fallback,
                fail_stream: true,
                stream_calls: Mutex::new(0),
                complete_calls: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl LlmClient for StreamingTestClient {
        async fn complete(&self, _request: LlmRequest) -> Result<LlmResponse> {
            *self.complete_calls.lock().unwrap() += 1;
            Ok(self.fallback.clone())
        }

        async fn stream(&self, _request: LlmRequest) -> Result<liquid_llm::LlmStream> {
            *self.stream_calls.lock().unwrap() += 1;
            if self.fail_stream {
                return Err(anyhow!("stream failed"));
            }

            let events = self
                .events
                .lock()
                .unwrap()
                .drain(..)
                .map(Ok)
                .collect::<Vec<_>>();
            Ok(Box::pin(stream::iter(events)))
        }
    }

    fn request() -> LlmRequest {
        LlmRequest::new(
            "gpt-test",
            LlmProtocol::ChatCompletions,
            vec![LlmMessage::user("hello")],
        )
    }

    #[tokio::test]
    async fn streaming_invocation_collects_text_and_emits_deltas() {
        let client = Arc::new(StreamingTestClient::new(
            vec![
                LlmEvent::TextDelta("hel".to_owned()),
                LlmEvent::TextDelta("lo".to_owned()),
                LlmEvent::Done,
            ],
            LlmResponse::text("fallback"),
        ));
        let deltas = Arc::new(Mutex::new(Vec::new()));
        let captured = deltas.clone();

        let response = invoke_llm_with_text_delta(
            &(client.clone() as Arc<dyn LlmClient>),
            request(),
            LlmInvocationMode::StreamWithFallback,
            move |delta| {
                let captured = captured.clone();
                async move {
                    captured.lock().unwrap().push(delta);
                }
            },
        )
        .await
        .unwrap();

        assert_eq!(response.content, "hello");
        assert_eq!(*client.stream_calls.lock().unwrap(), 1);
        assert_eq!(*client.complete_calls.lock().unwrap(), 0);
        assert_eq!(
            *deltas.lock().unwrap(),
            vec!["hel".to_owned(), "lo".to_owned()]
        );
    }

    #[tokio::test]
    async fn streaming_invocation_aggregates_tool_call_deltas_by_index() {
        let client = Arc::new(StreamingTestClient::new(
            vec![
                LlmEvent::ToolCallDelta {
                    index: Some(0),
                    id: Some("call_1".to_owned()),
                    name: Some("first".to_owned()),
                    arguments_delta: "{\"value\"".to_owned(),
                },
                LlmEvent::ToolCallDelta {
                    index: Some(0),
                    id: None,
                    name: None,
                    arguments_delta: ":1}".to_owned(),
                },
                LlmEvent::ToolCallDelta {
                    index: Some(1),
                    id: Some("call_2".to_owned()),
                    name: Some("second".to_owned()),
                    arguments_delta: "{\"value\":2}".to_owned(),
                },
                LlmEvent::Done,
            ],
            LlmResponse::text("fallback"),
        ));

        let response = invoke_llm(
            &(client as Arc<dyn LlmClient>),
            request(),
            LlmInvocationMode::StreamWithFallback,
        )
        .await
        .unwrap();

        assert_eq!(response.tool_calls.len(), 2);
        assert_eq!(response.tool_calls[0].id, "call_1");
        assert_eq!(response.tool_calls[0].name, "first");
        assert_eq!(response.tool_calls[0].arguments, r#"{"value":1}"#);
        assert_eq!(response.tool_calls[1].id, "call_2");
        assert_eq!(response.tool_calls[1].name, "second");
        assert_eq!(response.tool_calls[1].arguments, r#"{"value":2}"#);
    }

    #[tokio::test]
    async fn streaming_invocation_falls_back_to_complete_when_stream_fails() {
        let client = Arc::new(StreamingTestClient::failing_stream(LlmResponse::text(
            "fallback",
        )));

        let response = invoke_llm(
            &(client.clone() as Arc<dyn LlmClient>),
            request(),
            LlmInvocationMode::StreamWithFallback,
        )
        .await
        .unwrap();

        assert_eq!(response.content, "fallback");
        assert_eq!(*client.stream_calls.lock().unwrap(), 1);
        assert_eq!(*client.complete_calls.lock().unwrap(), 1);
    }
}
