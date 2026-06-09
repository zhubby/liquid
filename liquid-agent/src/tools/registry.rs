use std::{collections::BTreeMap, sync::Arc};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use liquid_llm::{ToolCall, ToolDefinition};
use serde_json::Value;

use crate::types::ToolOutput;

#[async_trait]
pub trait AgentTool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, arguments: Value) -> Result<ToolOutput>;
}

#[derive(Clone, Default)]
pub struct ToolRegistry {
    pub(crate) tools: BTreeMap<String, Arc<dyn AgentTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<T>(&mut self, tool: T)
    where
        T: AgentTool + 'static,
    {
        let definition = tool.definition();
        self.tools.insert(definition.name, Arc::new(tool));
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|tool| tool.definition()).collect()
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub async fn execute(&self, call: &ToolCall) -> Result<ToolOutput> {
        let tool = self
            .tools
            .get(&call.name)
            .ok_or_else(|| anyhow!("unknown agent tool: {}", call.name))?;
        let arguments = call.json_arguments()?;

        tool.execute(arguments)
            .await
            .with_context(|| format!("agent tool failed: {}", call.name))
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use liquid_llm::ToolCall;
    use serde_json::{Value, json};

    use super::*;

    #[derive(Default)]
    struct EchoTool;

    #[async_trait]
    impl AgentTool for EchoTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new(
                "echo_tool",
                "Echo a value.",
                json!({
                    "type": "object",
                    "properties": {
                        "value": { "type": "string" }
                    },
                    "required": ["value"],
                    "additionalProperties": false
                }),
            )
        }

        async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
            Ok(ToolOutput::json(json!({
                "value": arguments.get("value").and_then(Value::as_str).unwrap_or_default()
            })))
        }
    }

    #[tokio::test]
    async fn tool_registry_executes_registered_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(EchoTool);

        let output = registry
            .execute(&ToolCall::new("call_1", "echo_tool", r#"{"value":"ok"}"#))
            .await
            .unwrap();

        assert_eq!(output.content, r#"{"value":"ok"}"#);
    }

    #[tokio::test]
    async fn tool_registry_rejects_unknown_tool() {
        let registry = ToolRegistry::new();
        let error = registry
            .execute(&ToolCall::new("call_1", "missing_tool", "{}"))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("unknown agent tool"));
    }

    #[test]
    fn tool_registry_lists_registered_tool_names() {
        let mut registry = ToolRegistry::new();
        registry.register(EchoTool);

        assert_eq!(registry.tool_names(), vec!["echo_tool"]);
    }
}
