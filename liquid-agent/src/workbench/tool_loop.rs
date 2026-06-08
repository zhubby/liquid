use std::{future::Future, time::Instant};

use anyhow::{Result, bail};
use liquid_llm::{LlmMessage, LlmRequest, ToolCall};

use crate::{
    llm_invocation::{invoke_llm, invoke_llm_with_text_delta},
    tools::{ToolRegistry, execution::failed_tool_output},
};

use super::{
    LlmWorkbenchAgent, LlmWorkbenchContext,
    prompt::{WORKBENCH_SYSTEM_PROMPT, workbench_context_payload},
    proposal_tools::{
        is_workbench_proposal_tool, proposal_tool_call_to_suggestion,
        register_workbench_proposal_tools,
    },
    response::{WorkbenchResponse, WorkbenchToolStep, parse_llm_workbench_response},
};

pub(super) async fn run_tool_loop(
    agent: &LlmWorkbenchAgent,
    context: LlmWorkbenchContext,
    mut messages: Vec<LlmMessage>,
    tools: ToolRegistry,
    max_output_tokens: u32,
) -> Result<WorkbenchResponse> {
    let mut tool_steps = Vec::new();

    for _ in 0..agent.max_tool_rounds {
        let response = invoke_llm(
            &agent.llm,
            llm_request(agent, messages.clone(), &tools, max_output_tokens),
            agent.invocation_mode,
        )
        .await?;

        if response.tool_calls.is_empty() {
            return Ok(parse_llm_workbench_response(&response.content, &context)?
                .with_tool_steps(tool_steps));
        }

        messages.push(LlmMessage::assistant_with_response_items(
            response.content.clone(),
            response.tool_calls.clone(),
            response.output_items.clone(),
        ));

        let mut proposals = Vec::new();
        for call in &response.tool_calls {
            let step = execute_workbench_tool_for_model(&tools, call, &context).await?;
            messages.push(LlmMessage::tool_result(
                call.id.clone(),
                step.output.content.clone(),
            ));

            if let Some(proposal) = step.proposal.clone() {
                proposals.push(proposal);
            }

            tool_steps.push(step);
        }

        if !proposals.is_empty() {
            let response = invoke_llm(
                &agent.llm,
                no_tool_llm_request(agent, messages.clone(), max_output_tokens),
                agent.invocation_mode,
            )
            .await?;

            if !response.tool_calls.is_empty() {
                bail!(
                    "LLM workbench response requested tools after creating a confirmation proposal"
                );
            }

            let mut parsed = parse_llm_workbench_response(&response.content, &context)?;
            parsed.actions.splice(0..0, proposals);
            parsed.waiting_for_user = !parsed.actions.is_empty();
            parsed.tool_steps = tool_steps;
            return Ok(parsed);
        }
    }

    bail!(
        "LLM workbench exceeded maximum tool rounds ({})",
        agent.max_tool_rounds
    )
}

pub(super) async fn run_tool_loop_with_text_delta<F, Fut>(
    agent: &LlmWorkbenchAgent,
    context: LlmWorkbenchContext,
    mut tools: ToolRegistry,
    mut on_text_delta: F,
) -> Result<WorkbenchResponse>
where
    F: FnMut(String) -> Fut + Send,
    Fut: Future<Output = ()> + Send,
{
    register_workbench_proposal_tools(&mut tools);
    let mut messages = vec![
        LlmMessage::system(WORKBENCH_SYSTEM_PROMPT),
        LlmMessage::user(workbench_context_payload(&context)?),
    ];
    let mut tool_steps = Vec::new();
    let max_output_tokens = 1_200;

    for _ in 0..agent.max_tool_rounds {
        let response = invoke_llm_with_text_delta(
            &agent.llm,
            llm_request(agent, messages.clone(), &tools, max_output_tokens),
            agent.invocation_mode,
            &mut on_text_delta,
        )
        .await?;

        if response.tool_calls.is_empty() {
            return Ok(parse_llm_workbench_response(&response.content, &context)?
                .with_tool_steps(tool_steps));
        }

        messages.push(LlmMessage::assistant_with_response_items(
            response.content.clone(),
            response.tool_calls.clone(),
            response.output_items.clone(),
        ));

        let mut proposals = Vec::new();
        for call in &response.tool_calls {
            let step = execute_workbench_tool_for_model(&tools, call, &context).await?;
            messages.push(LlmMessage::tool_result(
                call.id.clone(),
                step.output.content.clone(),
            ));

            if let Some(proposal) = step.proposal.clone() {
                proposals.push(proposal);
            }

            tool_steps.push(step);
        }

        if !proposals.is_empty() {
            let response = invoke_llm_with_text_delta(
                &agent.llm,
                no_tool_llm_request(agent, messages.clone(), max_output_tokens),
                agent.invocation_mode,
                &mut on_text_delta,
            )
            .await?;

            if !response.tool_calls.is_empty() {
                bail!(
                    "LLM workbench response requested tools after creating a confirmation proposal"
                );
            }

            let mut parsed = parse_llm_workbench_response(&response.content, &context)?;
            parsed.actions.splice(0..0, proposals);
            parsed.waiting_for_user = !parsed.actions.is_empty();
            parsed.tool_steps = tool_steps;
            return Ok(parsed);
        }
    }

    bail!(
        "LLM workbench exceeded maximum tool rounds ({})",
        agent.max_tool_rounds
    )
}

fn llm_request(
    agent: &LlmWorkbenchAgent,
    messages: Vec<LlmMessage>,
    tools: &ToolRegistry,
    max_output_tokens: u32,
) -> LlmRequest {
    LlmRequest::new(agent.model.clone(), agent.protocol, messages)
        .with_tools(tools.definitions())
        .with_temperature(0.2)
        .with_max_output_tokens(max_output_tokens)
}

fn no_tool_llm_request(
    agent: &LlmWorkbenchAgent,
    messages: Vec<LlmMessage>,
    max_output_tokens: u32,
) -> LlmRequest {
    LlmRequest::new(agent.model.clone(), agent.protocol, messages)
        .with_temperature(0.2)
        .with_max_output_tokens(max_output_tokens)
}

async fn execute_workbench_tool_for_model(
    tools: &ToolRegistry,
    call: &ToolCall,
    context: &LlmWorkbenchContext,
) -> Result<WorkbenchToolStep> {
    if !tools.contains(&call.name) {
        bail!("unsupported workbench tool: {}", call.name);
    }

    let started_at = Instant::now();
    let arguments = call.json_arguments()?;

    if is_workbench_proposal_tool(&call.name) {
        let output = tools.execute(call).await?;
        let proposal = proposal_tool_call_to_suggestion(call, context)?;

        return Ok(WorkbenchToolStep {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments,
            output,
            succeeded: true,
            elapsed_ms: elapsed_ms(started_at),
            proposal: Some(proposal),
        });
    }

    match tools.execute(call).await {
        Ok(output) => Ok(WorkbenchToolStep {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments,
            output,
            succeeded: true,
            elapsed_ms: elapsed_ms(started_at),
            proposal: None,
        }),
        Err(error) => Ok(WorkbenchToolStep {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments,
            output: failed_tool_output(call, error, "workbench_agent"),
            succeeded: false,
            elapsed_ms: elapsed_ms(started_at),
            proposal: None,
        }),
    }
}

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis() as u64
}
