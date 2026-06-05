create table if not exists sql_audits (
    id uuid primary key default gen_random_uuid(),
    owner_user_id uuid not null references users(id) on delete cascade,
    managed_database_id uuid not null references managed_databases(id) on delete cascade,
    managed_database_name text not null,
    managed_database_engine text not null,
    managed_database_host text not null,
    managed_database_port integer not null,
    managed_database_database text not null,
    managed_database_username text not null,
    managed_database_ssl_mode text not null,
    sql text not null,
    schema text,
    context text,
    execution_purpose text,
    status text not null,
    statement_kind text,
    risk_score integer not null default 0,
    report jsonb,
    deterministic_analysis jsonb,
    approved_by_user_id uuid references users(id) on delete set null,
    approved_at timestamptz,
    approval_comment text,
    rejected_by_user_id uuid references users(id) on delete set null,
    rejected_at timestamptz,
    rejection_comment text,
    execution_result jsonb,
    execution_error text,
    executed_at timestamptz,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint sql_audits_sql_not_blank check (length(trim(sql)) > 0),
    constraint sql_audits_status_check check (
        status in (
            'audited',
            'pending_approval',
            'approved',
            'rejected',
            'blocked',
            'executing',
            'executed',
            'execution_failed'
        )
    ),
    constraint sql_audits_statement_kind_check check (
        statement_kind is null or statement_kind in (
            'select',
            'insert',
            'update',
            'delete',
            'merge',
            'create',
            'alter',
            'drop',
            'truncate',
            'security',
            'transaction',
            'control',
            'other'
        )
    ),
    constraint sql_audits_risk_score_check check (risk_score between 0 and 100),
    constraint sql_audits_execution_purpose_not_blank check (
        execution_purpose is null or length(trim(execution_purpose)) > 0
    ),
    constraint sql_audits_managed_database_engine_check check (managed_database_engine = 'postgres'),
    constraint sql_audits_managed_database_ssl_mode_check check (
        managed_database_ssl_mode in ('disable', 'prefer', 'require')
    )
);

create index if not exists sql_audits_owner_created_at_idx
    on sql_audits (owner_user_id, created_at desc);

create index if not exists sql_audits_owner_database_created_at_idx
    on sql_audits (owner_user_id, managed_database_id, created_at desc);

create index if not exists sql_audits_owner_status_created_at_idx
    on sql_audits (owner_user_id, status, created_at desc);

create table if not exists sql_audit_events (
    id uuid primary key default gen_random_uuid(),
    sql_audit_id uuid not null references sql_audits(id) on delete cascade,
    owner_user_id uuid not null references users(id) on delete cascade,
    event_type text not null,
    actor_user_id uuid references users(id) on delete set null,
    message text,
    payload jsonb,
    created_at timestamptz not null default now(),
    constraint sql_audit_events_type_check check (
        event_type in (
            'created',
            'audited',
            'blocked',
            'approved',
            'rejected',
            'execution_started',
            'executed',
            'execution_failed'
        )
    )
);

create index if not exists sql_audit_events_audit_created_at_idx
    on sql_audit_events (sql_audit_id, created_at);
