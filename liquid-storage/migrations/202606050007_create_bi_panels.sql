create table if not exists bi_panels (
    id uuid primary key default gen_random_uuid(),
    conversation_id uuid not null references agent_conversations(id) on delete cascade,
    owner_user_id uuid not null references users(id) on delete cascade,
    title text not null,
    description text,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint bi_panels_title_not_blank check (length(trim(title)) > 0),
    constraint bi_panels_description_not_blank check (
        description is null or length(trim(description)) > 0
    )
);

create unique index if not exists bi_panels_conversation_unique_idx
    on bi_panels (conversation_id);

create index if not exists bi_panels_owner_updated_at_idx
    on bi_panels (owner_user_id, updated_at desc);

create table if not exists bi_panel_cards (
    id uuid primary key default gen_random_uuid(),
    panel_id uuid not null references bi_panels(id) on delete cascade,
    owner_user_id uuid not null references users(id) on delete cascade,
    managed_database_id uuid not null references managed_databases(id) on delete cascade,
    source_action_id uuid references agent_actions(id) on delete set null,
    title text not null,
    description text,
    kind text not null,
    sql text not null,
    chart jsonb,
    layout jsonb not null,
    result jsonb not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint bi_panel_cards_kind_check check (kind in ('table', 'chart')),
    constraint bi_panel_cards_title_not_blank check (length(trim(title)) > 0),
    constraint bi_panel_cards_description_not_blank check (
        description is null or length(trim(description)) > 0
    ),
    constraint bi_panel_cards_sql_not_blank check (length(trim(sql)) > 0)
);

create index if not exists bi_panel_cards_panel_updated_at_idx
    on bi_panel_cards (panel_id, updated_at desc);

alter table agent_actions
    drop constraint if exists agent_actions_kind_check;

alter table agent_actions
    add constraint agent_actions_kind_check check (
        kind in (
            'create_sql_audit',
            'create_bi_card',
            'approve_sql_audit',
            'reject_sql_audit',
            'execute_sql_audit',
            'create_managed_database',
            'update_managed_database',
            'delete_managed_database',
            'start_database_backup',
            'start_database_restore'
        )
    );

alter table agent_actions
    drop constraint if exists agent_actions_resource_kind_check;

alter table agent_actions
    add constraint agent_actions_resource_kind_check check (
        resource_kind is null or resource_kind in (
            'sql_audit',
            'bi_panel_card',
            'managed_database',
            'database_backup',
            'database_restore'
        )
    );
