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
    let message = format_error_chain(&error);
    tracing::error!(
        tool_name = %call.name,
        tool_call_id = %call.id,
        error = %message,
        error_debug = ?error,
        caller,
        "agent tool call failed; returning error to model"
    );
    ToolOutput::json(json!({
        "ok": false,
        "tool": call.name,
        "error": message,
    }))
}

fn format_error_chain(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

#[cfg(test)]
mod tests {
    use liquid_llm::ToolCall;

    use super::*;

    #[test]
    fn failed_tool_output_includes_error_chain() {
        let error = anyhow::anyhow!("inner failure").context("outer context");
        let output = failed_tool_output(
            &ToolCall::new("call_1", "pg_describe_relation", "{}"),
            error,
            "test_agent",
        );
        let payload: serde_json::Value = serde_json::from_str(&output.content).unwrap();

        assert_eq!(payload["ok"], false);
        assert_eq!(payload["tool"], "pg_describe_relation");
        assert_eq!(payload["error"], "outer context: inner failure");
    }
}
