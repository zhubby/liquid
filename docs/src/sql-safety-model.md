# SQL Safety Model

Liquid treats SQL text as untrusted input. The safety model combines
deterministic PostgreSQL parsing, local risk rules, execution-mode configuration,
managed database snapshots, and explicit user confirmation.

## Deterministic Analysis

`liquid-sql` is the first safety layer. It uses `pg_query` to parse PostgreSQL
and produces a `PgSqlAnalysis` containing:

- parsed statement list,
- statement kind,
- source location and length when available,
- deterministic findings,
- optional parse error,
- optional metadata.

Empty SQL and PostgreSQL parse failures produce high-severity `parse_error`
findings.

Statement kinds are normalized to:

- `select`
- `insert`
- `update`
- `delete`
- `merge`
- `create`
- `alter`
- `drop`
- `truncate`
- `security`
- `transaction`
- `control`
- `other`

## Rule Areas

The rule modules inspect top-level and nested AST nodes:

| Module | Examples of Covered Risk |
| --- | --- |
| `query` | SELECT behavior such as row locking. |
| `dml` | INSERT, UPDATE, DELETE, and MERGE risk. |
| `ddl` | DROP, TRUNCATE, ALTER, CREATE, extension/function/index operations. |
| `security` | GRANT, role grants, role changes, role drops. |
| `control` | COPY, LOCK, DO blocks, transaction controls, materialized view refresh. |

Rules produce findings with severity, title, detail, optional statement index,
and optional evidence. The audit lifecycle uses deterministic findings even when
an LLM audit report is available.

## Audit Status Decisions

SQL audit creation chooses initial status from deterministic analysis:

| Condition | Status |
| --- | --- |
| Any critical deterministic finding | `blocked` |
| Exactly one SELECT statement | `audited` |
| Exactly one transaction or control statement | `blocked` |
| Exactly one non-SELECT statement with non-empty execution purpose | `pending_approval` |
| Exactly one non-SELECT statement without execution purpose | `audited` |
| Multiple statements or unknown statement kind | `blocked` |

The stored risk score is `max(deterministic_risk_floor, model_report.risk_score)`.

If the LLM returns an invalid JSON audit report, Liquid records a deterministic
fallback report instead of discarding the audit. The fallback includes a low-risk
finding explaining that the model report was unavailable.

## Execution Modes

`LIQUID_SQL_EXECUTION` controls PostgreSQL audit tools:

| Mode | Behavior |
| --- | --- |
| `off` | Managed audit execution tools are disabled. |
| `readonly` | Read-only execution tools are available. This is the default. |
| `write_gated` | Read-only tools plus gated write execution are available. Approved write audit execution requires this mode. |

Workbench chat tools always use the read-only PostgreSQL tool set. They do not
receive write execution tools.

## Metadata Modes

`LIQUID_SQL_METADATA` controls whether metadata collection is optional:

| Mode | Behavior |
| --- | --- |
| `auto` | Try managed database metadata when a pool exists, but audits can continue without it. |
| `off` | Do not require metadata collection. |
| `required` | Metadata tool failures fail the audit path that requires metadata. |

The API maps `required` to `PostgresToolConfig.metadata_required = true`.

## Datapanel Query Safety

Datapanel materialization is intentionally narrow:

1. SQL must be non-empty.
2. It must parse as exactly one PostgreSQL statement.
3. The statement must be SELECT.
4. SELECTs with row-locking findings are rejected.
5. A trailing semicolon is stripped.
6. The query is wrapped as:

   ```sql
   select to_jsonb(liquid_row) as row
   from (<user select>) liquid_row
   limit <fetch_limit>
   ```

7. Liquid starts a transaction and runs `set transaction read only`.
8. Liquid sets `statement_timeout = '5s'`.
9. The transaction is rolled back after reading rows.
10. Results are truncated to the requested limit, clamped to `1..1000`.

This path is used by datapanel card refresh, saving SQL mode SELECT results, and
read-only SQL audit execution.

## SQL Mode Execution Safety

Direct SQL mode has a different risk profile because it can commit accepted
mutation statements.

Validation:

- SQL must be non-empty.
- SQL must parse.
- There must be exactly one statement.
- Transaction and control statements are rejected.

Execution:

- SELECT statements use the datapanel read-only materialization path and are
  saveable as datapanel cards.
- Mutation statements with `RETURNING` are wrapped in a CTE and materialized as a
  query result, then committed. They are not saveable as datapanel cards.
- Other accepted statements execute directly and return a summary with affected
  rows.
- Mutation execution sets `statement_timeout = '5s'` and `lock_timeout = '5s'`.

Because SQL mode can commit, UI placement and user education should treat it as
an operator tool, not as a preview-only query box.

## Approved Write Execution Safety

Approved write execution goes through SQL audit records:

1. The audit must still reference an existing managed database.
2. The stored managed database snapshot must match the current managed database
   record.
3. `LIQUID_SQL_EXECUTION` must be `write_gated`.
4. The audit execution status moves to `executing`.
5. The write executor performs its own deterministic checks.
6. Success stores `SqlAuditExecutionResult`.
7. Failure stores `execution_error`.

Snapshot matching prevents a user from auditing SQL against one connection and
executing it later against a changed host, database name, username, or SSL mode.

## LLM Safety Boundary

The LLM is not the sole decision maker:

- deterministic parsing runs before model output is persisted,
- deterministic critical findings block audits,
- model risk score cannot lower the deterministic risk floor,
- invalid model JSON falls back to deterministic findings,
- workbench state changes require explicit actions,
- write execution is controlled by backend configuration and audit state.

Provider output may improve explanations and recommendations, but backend
lifecycles are governed by typed status transitions and deterministic gates.
