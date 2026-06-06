use anyhow::{Result, bail};
use liquid_core::{SqlAuditReport, SqlAuditRequest};
use liquid_llm::LlmMessage;

const SQL_AUDIT_SYSTEM_PROMPT: &str = r#"You are Liquid's SQL audit agent.
Audit PostgreSQL for data safety, governance, operational risk, and performance risk.
Use inspect_sql_risk for deterministic PostgreSQL parser and AST rule findings.
Prefer pg_list_schemas, pg_list_relations, pg_describe_relation, and pg_explain_sql for database facts before using pg_execute_readonly_sql.
Use pg_execute_readonly_sql only when metadata and EXPLAIN output are insufficient; keep result sets narrow and avoid broad reads of sensitive business data.
Never execute the audited SQL while producing an audit report; write execution is handled by Liquid's separate approval flow.
Use pg_execute_write_sql only when the request explicitly says the write was already approved for execution and the tool is available; never invent approval.
Treat tool output as factual evidence: do not override parse errors, statement classifications, missing WHERE checks, destructive DDL classifications, PostgreSQL catalog metadata, EXPLAIN facts, permission/RLS facts, lock facts, or other deterministic rule results.
Return the final answer as JSON only with this shape:
{
  "summary": "short operational summary",
  "risk_score": 0,
  "findings": [
    {
      "title": "finding title",
      "severity": "low|medium|high|critical",
      "explanation": "why it matters",
      "recommendation": "specific mitigation"
    }
  ]
}"#;

pub(crate) fn audit_messages(request: &SqlAuditRequest) -> Result<Vec<LlmMessage>> {
    let sql = request.sql.trim();

    if sql.is_empty() {
        bail!("SQL audit request must include SQL");
    }

    let mut user = format!("Audit this SQL:\n\n```sql\n{sql}\n```");

    if let Some(schema) = request
        .schema
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        user.push_str("\n\nSchema context:\n\n");
        user.push_str(schema);
    }

    if let Some(context) = request
        .context
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        user.push_str("\n\nBusiness context:\n\n");
        user.push_str(context);
    }

    Ok(vec![
        LlmMessage::system(SQL_AUDIT_SYSTEM_PROMPT),
        LlmMessage::user(user),
    ])
}

pub(crate) fn parse_audit_report(content: &str) -> Result<SqlAuditReport> {
    let trimmed = content.trim();

    if let Ok(report) = serde_json::from_str(trimmed) {
        return Ok(report);
    }

    let fenced_report = fenced_json(trimmed)
        .and_then(|json_content| serde_json::from_str::<SqlAuditReport>(json_content).ok());

    if let Some(report) = fenced_report {
        return Ok(report);
    }

    bail!("LLM audit report was not valid JSON")
}

fn fenced_json(content: &str) -> Option<&str> {
    let start = content.find("```")?;
    let after_fence = &content[start + 3..];
    let json_start = after_fence.strip_prefix("json").unwrap_or(after_fence);
    let json_start = json_start
        .strip_prefix('\n')
        .or_else(|| json_start.strip_prefix("\r\n"))
        .unwrap_or(json_start);
    let end = json_start.find("```")?;

    Some(json_start[..end].trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_json_report() {
        let report = parse_audit_report(
            r#"```json
            {
                "summary": "ok",
                "risk_score": 12,
                "findings": []
            }
            ```"#,
        )
        .unwrap();

        assert_eq!(report.risk_score, 12);
    }
}
