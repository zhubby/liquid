create table if not exists agent_conversations (
    id uuid primary key default gen_random_uuid(),
    owner_user_id uuid not null references users(id) on delete cascade,
    title text not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint agent_conversations_title_not_blank check (length(trim(title)) > 0)
);

create index if not exists agent_conversations_owner_updated_at_idx
    on agent_conversations (owner_user_id, updated_at desc);

create table if not exists agent_messages (
    id uuid primary key default gen_random_uuid(),
    conversation_id uuid not null references agent_conversations(id) on delete cascade,
    owner_user_id uuid not null references users(id) on delete cascade,
    turn_id uuid,
    role text not null,
    content text not null,
    metadata jsonb,
    created_at timestamptz not null default now(),
    constraint agent_messages_role_check check (role in ('user', 'assistant', 'tool', 'system')),
    constraint agent_messages_content_not_blank check (length(trim(content)) > 0)
);

create index if not exists agent_messages_conversation_created_at_idx
    on agent_messages (conversation_id, created_at, id);

create table if not exists agent_turns (
    id uuid primary key default gen_random_uuid(),
    conversation_id uuid not null references agent_conversations(id) on delete cascade,
    owner_user_id uuid not null references users(id) on delete cascade,
    status text not null default 'queued',
    user_message_id uuid not null references agent_messages(id) on delete cascade,
    assistant_message_id uuid references agent_messages(id) on delete set null,
    error text,
    client_request_id text,
    managed_database_id uuid references managed_databases(id) on delete set null,
    dashboard_context jsonb,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    completed_at timestamptz,
    constraint agent_turns_status_check check (
        status in ('queued', 'running', 'completed', 'blocked', 'failed', 'cancelled')
    ),
    constraint agent_turns_client_request_id_not_blank check (
        client_request_id is null or length(trim(client_request_id)) > 0
    )
);

create index if not exists agent_turns_owner_created_at_idx
    on agent_turns (owner_user_id, created_at desc);

create index if not exists agent_turns_conversation_created_at_idx
    on agent_turns (conversation_id, created_at desc);

create table if not exists agent_turn_events (
    id uuid primary key default gen_random_uuid(),
    turn_id uuid not null references agent_turns(id) on delete cascade,
    owner_user_id uuid not null references users(id) on delete cascade,
    seq integer not null,
    event_type text not null,
    payload jsonb not null default '{}'::jsonb,
    created_at timestamptz not null default now(),
    constraint agent_turn_events_seq_positive check (seq > 0),
    constraint agent_turn_events_type_check check (
        event_type in (
            'turn_started',
            'message_created',
            'assistant_delta',
            'tool_call_started',
            'tool_call_finished',
            'resource_created',
            'resource_updated',
            'action_proposed',
            'turn_completed',
            'turn_failed'
        )
    ),
    unique (turn_id, seq)
);

create index if not exists agent_turn_events_turn_seq_idx
    on agent_turn_events (turn_id, seq);

create table if not exists agent_actions (
    id uuid primary key default gen_random_uuid(),
    conversation_id uuid not null references agent_conversations(id) on delete cascade,
    turn_id uuid not null references agent_turns(id) on delete cascade,
    owner_user_id uuid not null references users(id) on delete cascade,
    kind text not null,
    status text not null default 'proposed',
    title text not null,
    description text not null,
    payload jsonb not null default '{}'::jsonb,
    resource_kind text,
    resource_id uuid,
    requires_confirmation boolean not null default true,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint agent_actions_kind_check check (
        kind in (
            'create_sql_audit',
            'approve_sql_audit',
            'reject_sql_audit',
            'execute_sql_audit',
            'create_managed_database',
            'update_managed_database',
            'delete_managed_database',
            'start_database_backup',
            'start_database_restore'
        )
    ),
    constraint agent_actions_status_check check (
        status in ('proposed', 'applied', 'rejected', 'failed', 'superseded')
    ),
    constraint agent_actions_resource_kind_check check (
        resource_kind is null or resource_kind in (
            'sql_audit',
            'managed_database',
            'database_backup',
            'database_restore'
        )
    ),
    constraint agent_actions_title_not_blank check (length(trim(title)) > 0),
    constraint agent_actions_description_not_blank check (length(trim(description)) > 0)
);

create index if not exists agent_actions_owner_status_created_at_idx
    on agent_actions (owner_user_id, status, created_at desc);

create index if not exists agent_actions_conversation_status_created_at_idx
    on agent_actions (conversation_id, status, created_at desc);
