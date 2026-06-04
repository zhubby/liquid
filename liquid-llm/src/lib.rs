use std::pin::Pin;

use anyhow::{Context, Result, anyhow, bail};
use async_stream::try_stream;
use async_trait::async_trait;
use futures_core::Stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

const DEFAULT_BASE_URL: &str = "https://api.openai.com";

pub type LlmStream = Pin<Box<dyn Stream<Item = Result<LlmEvent>> + Send>>;

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse>;
    async fn stream(&self, request: LlmRequest) -> Result<LlmStream>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LlmProtocol {
    #[default]
    ChatCompletions,
    Responses,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmRequest {
    pub model: String,
    pub protocol: LlmProtocol,
    pub messages: Vec<LlmMessage>,
    pub tools: Vec<ToolDefinition>,
    pub temperature: Option<f32>,
    pub max_output_tokens: Option<u32>,
}

impl LlmRequest {
    pub fn new(model: impl Into<String>, protocol: LlmProtocol, messages: Vec<LlmMessage>) -> Self {
        Self {
            model: model.into(),
            protocol,
            messages,
            tools: Vec::new(),
            temperature: None,
            max_output_tokens: None,
        }
    }

    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_max_output_tokens(mut self, max_output_tokens: u32) -> Self {
        self.max_output_tokens = Some(max_output_tokens);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

impl MessageRole {
    fn as_wire_role(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: MessageRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
}

impl LlmMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(MessageRole::System, content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(MessageRole::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(MessageRole::Assistant, content)
    }

    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
    ) -> Self {
        Self {
            tool_calls,
            ..Self::assistant(content)
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
            name: None,
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: Vec::new(),
        }
    }

    fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            name: None,
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolDefinition {
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl ToolCall {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }

    pub fn json_arguments(&self) -> Result<Value> {
        serde_json::from_str(&self.arguments)
            .with_context(|| format!("invalid JSON arguments for tool {}", self.name))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmResponse {
    pub id: Option<String>,
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub raw: Value,
}

impl LlmResponse {
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            id: None,
            content: content.into(),
            tool_calls: Vec::new(),
            raw: Value::Null,
        }
    }

    pub fn with_tool_calls(mut self, tool_calls: Vec<ToolCall>) -> Self {
        self.tool_calls = tool_calls;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LlmEvent {
    TextDelta(String),
    ToolCallDelta {
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    ToolCall(ToolCall),
    MessageDone(LlmResponse),
    RawJson(Value),
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiCompatibleConfig {
    pub api_key: Option<String>,
    pub base_url: String,
}

impl OpenAiCompatibleConfig {
    pub fn new(api_key: Option<String>, base_url: impl Into<String>) -> Self {
        Self {
            api_key,
            base_url: base_url.into(),
        }
    }
}

impl Default for OpenAiCompatibleConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleClient {
    http: reqwest::Client,
    api_key: Option<String>,
    base_url: String,
}

impl OpenAiCompatibleClient {
    pub fn new(config: OpenAiCompatibleConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: config.api_key,
            base_url: config.base_url.trim_end_matches('/').to_owned(),
        }
    }

    fn endpoint(&self, path: &str) -> String {
        if self.base_url.ends_with("/v1") && path.starts_with("/v1/") {
            format!("{}{}", self.base_url, &path[3..])
        } else {
            format!("{}/{}", self.base_url, path.trim_start_matches('/'))
        }
    }

    async fn post_json(&self, path: &str, body: Value) -> Result<reqwest::Response> {
        let mut request = self.http.post(self.endpoint(path)).json(&body);

        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }

        let response = request.send().await.context("LLM request failed")?;
        let status = response.status();

        if !status.is_success() {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read error body>".to_owned());
            bail!("LLM request returned {status}: {error_body}");
        }

        Ok(response)
    }
}

#[async_trait]
impl LlmClient for OpenAiCompatibleClient {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
        let protocol = request.protocol;
        let body = request_body(&request, false);
        let path = protocol_path(protocol);
        let response = self.post_json(path, body).await?;
        let raw: Value = response
            .json()
            .await
            .context("failed to decode LLM response JSON")?;

        response_from_value(protocol, raw)
    }

    async fn stream(&self, request: LlmRequest) -> Result<LlmStream> {
        let protocol = request.protocol;
        let body = request_body(&request, true);
        let path = protocol_path(protocol);
        let response = self.post_json(path, body).await?;

        Ok(sse_events(response, protocol))
    }
}

fn protocol_path(protocol: LlmProtocol) -> &'static str {
    match protocol {
        LlmProtocol::ChatCompletions => "/v1/chat/completions",
        LlmProtocol::Responses => "/v1/responses",
    }
}

fn request_body(request: &LlmRequest, stream: bool) -> Value {
    match request.protocol {
        LlmProtocol::ChatCompletions => chat_completions_body(request, stream),
        LlmProtocol::Responses => responses_body(request, stream),
    }
}

fn chat_completions_body(request: &LlmRequest, stream: bool) -> Value {
    let mut body = Map::new();
    body.insert("model".to_owned(), Value::String(request.model.clone()));
    body.insert(
        "messages".to_owned(),
        Value::Array(request.messages.iter().map(chat_message).collect()),
    );
    body.insert("stream".to_owned(), Value::Bool(stream));

    if !request.tools.is_empty() {
        body.insert(
            "tools".to_owned(),
            Value::Array(request.tools.iter().map(chat_tool).collect()),
        );
        body.insert("tool_choice".to_owned(), Value::String("auto".to_owned()));
    }

    if let Some(temperature) = request.temperature {
        body.insert("temperature".to_owned(), json!(temperature));
    }

    if let Some(max_output_tokens) = request.max_output_tokens {
        body.insert("max_tokens".to_owned(), json!(max_output_tokens));
    }

    Value::Object(body)
}

fn chat_message(message: &LlmMessage) -> Value {
    let mut object = Map::new();
    object.insert(
        "role".to_owned(),
        Value::String(message.role.as_wire_role().to_owned()),
    );

    match message.role {
        MessageRole::Tool => {
            object.insert("content".to_owned(), Value::String(message.content.clone()));
            if let Some(tool_call_id) = &message.tool_call_id {
                object.insert(
                    "tool_call_id".to_owned(),
                    Value::String(tool_call_id.clone()),
                );
            }
        }
        MessageRole::Assistant if !message.tool_calls.is_empty() => {
            object.insert(
                "content".to_owned(),
                if message.content.is_empty() {
                    Value::Null
                } else {
                    Value::String(message.content.clone())
                },
            );
            object.insert(
                "tool_calls".to_owned(),
                Value::Array(message.tool_calls.iter().map(chat_tool_call).collect()),
            );
        }
        _ => {
            object.insert("content".to_owned(), Value::String(message.content.clone()));
        }
    }

    if let Some(name) = &message.name {
        object.insert("name".to_owned(), Value::String(name.clone()));
    }

    Value::Object(object)
}

fn chat_tool(tool: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.parameters,
        }
    })
}

fn chat_tool_call(tool_call: &ToolCall) -> Value {
    json!({
        "id": tool_call.id,
        "type": "function",
        "function": {
            "name": tool_call.name,
            "arguments": tool_call.arguments,
        }
    })
}

fn responses_body(request: &LlmRequest, stream: bool) -> Value {
    let mut body = Map::new();
    body.insert("model".to_owned(), Value::String(request.model.clone()));
    body.insert("stream".to_owned(), Value::Bool(stream));

    let mut instructions = Vec::new();
    let mut input = Vec::new();

    for message in &request.messages {
        if message.role == MessageRole::System {
            instructions.push(message.content.as_str());
        } else {
            input.extend(responses_input_items(message));
        }
    }

    if !instructions.is_empty() {
        body.insert(
            "instructions".to_owned(),
            Value::String(instructions.join("\n\n")),
        );
    }

    body.insert("input".to_owned(), Value::Array(input));

    if !request.tools.is_empty() {
        body.insert(
            "tools".to_owned(),
            Value::Array(request.tools.iter().map(responses_tool).collect()),
        );
        body.insert("tool_choice".to_owned(), Value::String("auto".to_owned()));
    }

    if let Some(temperature) = request.temperature {
        body.insert("temperature".to_owned(), json!(temperature));
    }

    if let Some(max_output_tokens) = request.max_output_tokens {
        body.insert("max_output_tokens".to_owned(), json!(max_output_tokens));
    }

    Value::Object(body)
}

fn responses_input_items(message: &LlmMessage) -> Vec<Value> {
    match message.role {
        MessageRole::Tool => message
            .tool_call_id
            .as_ref()
            .map(|tool_call_id| {
                vec![json!({
                    "type": "function_call_output",
                    "call_id": tool_call_id,
                    "output": message.content,
                })]
            })
            .unwrap_or_default(),
        MessageRole::Assistant if !message.tool_calls.is_empty() => {
            let mut items = Vec::new();

            if !message.content.is_empty() {
                items.push(json!({
                    "role": "assistant",
                    "content": message.content,
                }));
            }

            items.extend(message.tool_calls.iter().map(|tool_call| {
                json!({
                    "type": "function_call",
                    "call_id": tool_call.id,
                    "name": tool_call.name,
                    "arguments": tool_call.arguments,
                })
            }));

            items
        }
        MessageRole::System | MessageRole::User | MessageRole::Assistant => {
            vec![json!({
                "role": message.role.as_wire_role(),
                "content": message.content,
            })]
        }
    }
}

fn responses_tool(tool: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "name": tool.name,
        "description": tool.description,
        "parameters": tool.parameters,
    })
}

fn response_from_value(protocol: LlmProtocol, raw: Value) -> Result<LlmResponse> {
    match protocol {
        LlmProtocol::ChatCompletions => chat_response_from_value(raw),
        LlmProtocol::Responses => responses_response_from_value(raw),
    }
}

fn chat_response_from_value(raw: Value) -> Result<LlmResponse> {
    let message = raw
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .ok_or_else(|| anyhow!("chat completion response did not include a message"))?;

    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    let tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .enumerate()
                .map(|(index, call)| chat_tool_call_from_value(call, index))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(LlmResponse {
        id: raw.get("id").and_then(Value::as_str).map(str::to_owned),
        content,
        tool_calls,
        raw,
    })
}

fn chat_tool_call_from_value(call: &Value, index: usize) -> ToolCall {
    let function = call.get("function").unwrap_or(&Value::Null);
    ToolCall {
        id: call
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("tool_call_{index}")),
        name: function
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        arguments: function
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or("{}")
            .to_owned(),
    }
}

fn responses_response_from_value(raw: Value) -> Result<LlmResponse> {
    let content = raw
        .get("output_text")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| collect_responses_output_text(&raw));

    let tool_calls = raw
        .get("output")
        .and_then(Value::as_array)
        .map(|output| {
            output
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
                .enumerate()
                .map(|(index, item)| responses_tool_call_from_value(item, index))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(LlmResponse {
        id: raw.get("id").and_then(Value::as_str).map(str::to_owned),
        content,
        tool_calls,
        raw,
    })
}

fn responses_tool_call_from_value(item: &Value, index: usize) -> ToolCall {
    ToolCall {
        id: item
            .get("call_id")
            .or_else(|| item.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("tool_call_{index}")),
        name: item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        arguments: item
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or("{}")
            .to_owned(),
    }
}

fn collect_responses_output_text(raw: &Value) -> String {
    let Some(output) = raw.get("output").and_then(Value::as_array) else {
        return String::new();
    };

    let mut text = String::new();

    for item in output {
        let Some(content) = item.get("content").and_then(Value::as_array) else {
            continue;
        };

        for part in content {
            if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                text.push_str(part_text);
            } else if let Some(part_text) = part.get("output_text").and_then(Value::as_str) {
                text.push_str(part_text);
            }
        }
    }

    text
}

fn sse_events(response: reqwest::Response, protocol: LlmProtocol) -> LlmStream {
    Box::pin(try_stream! {
        let mut chunks = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.context("failed to read LLM stream chunk")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(frame) = next_sse_frame(&mut buffer) {
                if let Some(event) = event_from_sse_frame(protocol, &frame)? {
                    let done = matches!(event, LlmEvent::Done);
                    yield event;

                    if done {
                        return;
                    }
                }
            }
        }

        if !buffer.trim().is_empty() {
            let event = event_from_sse_frame(protocol, &buffer)?;
            if let Some(event) = event {
                yield event;
            }
        }
    })
}

fn next_sse_frame(buffer: &mut String) -> Option<String> {
    let lf = buffer.find("\n\n").map(|index| (index, 2));
    let crlf = buffer.find("\r\n\r\n").map(|index| (index, 4));

    let (index, separator_len) = match (lf, crlf) {
        (Some(left), Some(right)) if left.0 <= right.0 => left,
        (Some(_), Some(right)) => right,
        (Some(left), None) => left,
        (None, Some(right)) => right,
        (None, None) => return None,
    };

    let frame = buffer[..index].to_owned();
    buffer.drain(..index + separator_len);
    Some(frame)
}

fn event_from_sse_frame(protocol: LlmProtocol, frame: &str) -> Result<Option<LlmEvent>> {
    let payload = frame
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("data:"))
        .map(|data| data.strip_prefix(' ').unwrap_or(data))
        .collect::<Vec<_>>()
        .join("\n");

    if payload.is_empty() {
        return Ok(None);
    }

    if payload == "[DONE]" {
        return Ok(Some(LlmEvent::Done));
    }

    let raw: Value =
        serde_json::from_str(&payload).with_context(|| "failed to decode LLM stream event JSON")?;

    match protocol {
        LlmProtocol::ChatCompletions => Ok(Some(chat_event_from_value(raw))),
        LlmProtocol::Responses => Ok(Some(responses_event_from_value(raw))),
    }
}

fn chat_event_from_value(raw: Value) -> LlmEvent {
    let choice = raw
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first());
    let delta = choice.and_then(|choice| choice.get("delta"));

    if let Some(text) = delta
        .and_then(|delta| delta.get("content"))
        .and_then(Value::as_str)
    {
        return LlmEvent::TextDelta(text.to_owned());
    }

    if let Some(tool_delta) = delta
        .and_then(|delta| delta.get("tool_calls"))
        .and_then(Value::as_array)
        .and_then(|tool_calls| tool_calls.first())
    {
        let function = tool_delta.get("function").unwrap_or(&Value::Null);
        return LlmEvent::ToolCallDelta {
            id: tool_delta
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            name: function
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned),
            arguments_delta: function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        };
    }

    LlmEvent::RawJson(raw)
}

fn responses_event_from_value(raw: Value) -> LlmEvent {
    let event_type = raw.get("type").and_then(Value::as_str).unwrap_or_default();

    if event_type.ends_with(".delta")
        && let Some(delta) = raw.get("delta").and_then(Value::as_str)
    {
        if event_type.contains("function_call_arguments") {
            return LlmEvent::ToolCallDelta {
                id: raw
                    .get("item_id")
                    .or_else(|| raw.get("call_id"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                name: None,
                arguments_delta: delta.to_owned(),
            };
        }

        return LlmEvent::TextDelta(delta.to_owned());
    }

    if event_type == "response.output_item.done"
        && let Some(item) = raw.get("item")
        && item.get("type").and_then(Value::as_str) == Some("function_call")
    {
        return LlmEvent::ToolCall(responses_tool_call_from_value(item, 0));
    }

    if event_type == "response.completed"
        && let Some(response) = raw.get("response")
        && let Ok(done) = responses_response_from_value(response.clone())
    {
        return LlmEvent::MessageDone(done);
    }

    LlmEvent::RawJson(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_completions_body_maps_tools_and_tool_results() {
        let request = LlmRequest::new(
            "gpt-test",
            LlmProtocol::ChatCompletions,
            vec![
                LlmMessage::user("inspect this"),
                LlmMessage::assistant_with_tool_calls(
                    "",
                    vec![ToolCall::new(
                        "call_1",
                        "inspect_sql",
                        r#"{"sql":"select *"}"#,
                    )],
                ),
                LlmMessage::tool_result("call_1", r#"{"risk":"medium"}"#),
            ],
        )
        .with_tools(vec![ToolDefinition::new(
            "inspect_sql",
            "Inspect SQL",
            json!({"type":"object"}),
        )]);

        let body = chat_completions_body(&request, false);

        assert_eq!(body["model"], "gpt-test");
        assert_eq!(body["messages"][1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(body["messages"][2]["role"], "tool");
        assert_eq!(body["messages"][2]["tool_call_id"], "call_1");
        assert_eq!(body["tools"][0]["function"]["name"], "inspect_sql");
    }

    #[test]
    fn responses_body_maps_function_call_outputs() {
        let request = LlmRequest::new(
            "gpt-test",
            LlmProtocol::Responses,
            vec![
                LlmMessage::system("audit SQL"),
                LlmMessage::user("select * from users"),
                LlmMessage::tool_result("call_1", r#"{"risk":"medium"}"#),
            ],
        );

        let body = responses_body(&request, false);

        assert_eq!(body["instructions"], "audit SQL");
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][1]["type"], "function_call_output");
        assert_eq!(body["input"][1]["call_id"], "call_1");
    }

    #[test]
    fn chat_response_parses_tool_calls() {
        let raw = json!({
            "id": "chatcmpl_1",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "inspect_sql",
                            "arguments": "{\"sql\":\"select *\"}"
                        }
                    }]
                }
            }]
        });

        let response = chat_response_from_value(raw).unwrap();

        assert_eq!(response.id.as_deref(), Some("chatcmpl_1"));
        assert_eq!(response.tool_calls[0].name, "inspect_sql");
        assert_eq!(
            response.tool_calls[0].json_arguments().unwrap()["sql"],
            "select *"
        );
    }

    #[test]
    fn responses_response_parses_output_text_and_function_call() {
        let raw = json!({
            "id": "resp_1",
            "output_text": "{\"summary\":\"ok\"}",
            "output": [{
                "type": "function_call",
                "call_id": "call_1",
                "name": "inspect_sql",
                "arguments": "{\"sql\":\"select 1\"}"
            }]
        });

        let response = responses_response_from_value(raw).unwrap();

        assert_eq!(response.content, "{\"summary\":\"ok\"}");
        assert_eq!(response.tool_calls[0].id, "call_1");
    }

    #[test]
    fn stream_frame_maps_chat_text_delta() {
        let event = event_from_sse_frame(
            LlmProtocol::ChatCompletions,
            "event: message\ndata: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}",
        )
        .unwrap()
        .unwrap();

        assert_eq!(event, LlmEvent::TextDelta("hi".to_owned()));
    }
}
