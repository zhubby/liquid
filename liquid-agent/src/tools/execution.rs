use serde_json::json;

use crate::{tools::ToolRegistry, types::ToolOutput};

pub(crate) async fn execute_tool_for_model(
    tools: &ToolRegistry,
    call: &liquid_llm::ToolCall,
    caller: &'static str,
) -> ToolOutput {
    match tools.execute(call).await {
        Ok(output) => output,
        Err(error) => failed_tool_output(call, error, caller),
    }
}

pub(crate) fn failed_tool_output(
    call: &liquid_llm::ToolCall,
    error: anyhow::Error,
    caller: &'static str,
) -> ToolOutput {
    let message = error.to_string();
    tracing::warn!(
        tool_name = %call.name,
        tool_call_id = %call.id,
        error = %message,
        caller,
        "agent tool call failed; returning error to model"
    );
    ToolOutput::json(json!({
        "ok": false,
        "tool": call.name,
        "error": message,
    }))
}
