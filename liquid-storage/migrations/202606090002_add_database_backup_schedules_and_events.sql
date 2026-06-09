create table if not exists database_backup_schedules (
    id uuid primary key default gen_random_uuid(),
    owner_user_id uuid not null references users(id) on delete cascade,
    source_managed_database_id uuid not null,
    source_managed_database_name text not null,
    source_managed_database_engine text not null,
    source_managed_database_host text not null,
    source_managed_database_port integer not null,
    source_managed_database_database text not null,
    source_managed_database_username text not null,
    source_managed_database_ssl_mode text not null,
    cron_expression text not null,
    timezone text not null default 'UTC',
    status text not null default 'active',
    purpose text,
    keep_last integer,
    retention_days integer,
    conversation_id uuid references agent_conversations(id) on delete set null,
    created_from_turn_id uuid references agent_turns(id) on delete set null,
    next_run_at timestamptz not null,
    last_enqueued_at timestamptz,
    scheduler_id text,
    claimed_at timestamptz,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint database_backup_schedules_source_name_not_blank check (length(trim(source_managed_database_name)) > 0),
    constraint database_backup_schedules_source_engine_check check (source_managed_database_engine = 'postgres'),
    constraint database_backup_schedules_source_port_check check (source_managed_database_port between 1 and 65535),
    constraint database_backup_schedules_source_database_not_blank check (length(trim(source_managed_database_database)) > 0),
    constraint database_backup_schedules_source_username_not_blank check (length(trim(source_managed_database_username)) > 0),
    constraint database_backup_schedules_source_ssl_mode_check check (
        source_managed_database_ssl_mode in ('disable', 'prefer', 'require')
    ),
    constraint database_backup_schedules_cron_not_blank check (length(trim(cron_expression)) > 0),
    constraint database_backup_schedules_timezone_not_blank check (length(trim(timezone)) > 0),
    constraint database_backup_schedules_status_check check (
        status in ('active', 'paused', 'deleted')
    ),
    constraint database_backup_schedules_purpose_not_blank check (
        purpose is null or length(trim(purpose)) > 0
    ),
    constraint database_backup_schedules_keep_last_positive check (
        keep_last is null or keep_last > 0
    ),
    constraint database_backup_schedules_retention_days_positive check (
        retention_days is null or retention_days > 0
    )
);

create index if not exists database_backup_schedules_owner_created_at_idx
    on database_backup_schedules (owner_user_id, created_at desc);

create index if not exists database_backup_schedules_owner_source_created_at_idx
    on database_backup_schedules (owner_user_id, source_managed_database_id, created_at desc);

create index if not exists database_backup_schedules_due_idx
    on database_backup_schedules (status, next_run_at)
    where status = 'active';

alter table database_backups
    add column if not exists schedule_id uuid references database_backup_schedules(id) on delete set null,
    add column if not exists trigger text not null default 'immediate',
    add column if not exists scheduled_for timestamptz,
    add column if not exists conversation_id uuid references agent_conversations(id) on delete set null,
    add column if not exists created_from_turn_id uuid references agent_turns(id) on delete set null;

alter table database_backups
    add constraint database_backups_trigger_check check (
        trigger in ('immediate', 'cron')
    ),
    add constraint database_backups_schedule_trigger_check check (
        (trigger = 'cron' and schedule_id is not null and scheduled_for is not null)
        or (trigger = 'immediate' and scheduled_for is null)
    );

create unique index if not exists database_backups_schedule_scheduled_for_idx
    on database_backups (schedule_id, scheduled_for)
    where schedule_id is not null and scheduled_for is not null;

create index if not exists database_backups_conversation_created_at_idx
    on database_backups (conversation_id, created_at desc)
    where conversation_id is not null;

alter table database_restore_jobs
    add column if not exists conversation_id uuid references agent_conversations(id) on delete set null,
    add column if not exists created_from_turn_id uuid references agent_turns(id) on delete set null;

create index if not exists database_restore_jobs_conversation_created_at_idx
    on database_restore_jobs (conversation_id, created_at desc)
    where conversation_id is not null;

create table if not exists database_operation_events (
    id uuid primary key default gen_random_uuid(),
    owner_user_id uuid not null references users(id) on delete cascade,
    operation_kind text not null,
    operation_id uuid not null,
    event_type text not null,
    conversation_id uuid references agent_conversations(id) on delete set null,
    turn_id uuid references agent_turns(id) on delete set null,
    payload jsonb not null default '{}'::jsonb,
    delivered_at timestamptz,
    delivered_message_id uuid references agent_messages(id) on delete set null,
    created_at timestamptz not null default now(),
    constraint database_operation_events_kind_check check (
        operation_kind in ('backup', 'restore')
    ),
    constraint database_operation_events_type_check check (
        event_type in ('queued', 'succeeded', 'failed')
    )
);

create unique index if not exists database_operation_events_unique_idx
    on database_operation_events (operation_kind, operation_id, event_type);

create index if not exists database_operation_events_undelivered_idx
    on database_operation_events (created_at)
    where delivered_at is null and conversation_id is not null;
