create table if not exists database_backups (
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
    format text not null default 'postgres_custom',
    s3_bucket text,
    s3_key text,
    s3_version_id text,
    s3_etag text,
    size_bytes bigint,
    checksum_sha256 text,
    postgres_server_version text,
    pg_dump_version text,
    status text not null default 'queued',
    phase text not null default 'queued',
    progress_percent integer not null default 0,
    worker_id text,
    heartbeat_at timestamptz,
    started_at timestamptz,
    completed_at timestamptz,
    error text,
    purpose text,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint database_backups_source_name_not_blank check (length(trim(source_managed_database_name)) > 0),
    constraint database_backups_source_engine_check check (source_managed_database_engine = 'postgres'),
    constraint database_backups_source_port_check check (source_managed_database_port between 1 and 65535),
    constraint database_backups_source_database_not_blank check (length(trim(source_managed_database_database)) > 0),
    constraint database_backups_source_username_not_blank check (length(trim(source_managed_database_username)) > 0),
    constraint database_backups_source_ssl_mode_check check (
        source_managed_database_ssl_mode in ('disable', 'prefer', 'require')
    ),
    constraint database_backups_format_check check (format = 'postgres_custom'),
    constraint database_backups_status_check check (
        status in ('queued', 'running', 'succeeded', 'failed', 'deleted')
    ),
    constraint database_backups_phase_not_blank check (length(trim(phase)) > 0),
    constraint database_backups_progress_percent_check check (progress_percent between 0 and 100),
    constraint database_backups_object_complete_check check (
        (status <> 'succeeded')
        or (
            s3_bucket is not null
            and length(trim(s3_bucket)) > 0
            and s3_key is not null
            and length(trim(s3_key)) > 0
            and size_bytes is not null
            and size_bytes >= 0
            and checksum_sha256 is not null
            and length(trim(checksum_sha256)) > 0
        )
    ),
    constraint database_backups_purpose_not_blank check (
        purpose is null or length(trim(purpose)) > 0
    )
);

create index if not exists database_backups_owner_created_at_idx
    on database_backups (owner_user_id, created_at desc);

create index if not exists database_backups_owner_source_created_at_idx
    on database_backups (owner_user_id, source_managed_database_id, created_at desc);

create index if not exists database_backups_status_created_at_idx
    on database_backups (status, created_at);

create table if not exists database_restore_jobs (
    id uuid primary key default gen_random_uuid(),
    owner_user_id uuid not null references users(id) on delete cascade,
    backup_id uuid not null references database_backups(id) on delete restrict,
    target_managed_database_id uuid not null,
    target_managed_database_name text not null,
    target_managed_database_engine text not null,
    target_managed_database_host text not null,
    target_managed_database_port integer not null,
    target_managed_database_database text not null,
    target_managed_database_username text not null,
    target_managed_database_ssl_mode text not null,
    format text not null default 'postgres_custom',
    restore_options jsonb not null default '{}'::jsonb,
    status text not null default 'queued',
    phase text not null default 'queued',
    progress_percent integer not null default 0,
    worker_id text,
    heartbeat_at timestamptz,
    started_at timestamptz,
    completed_at timestamptz,
    error text,
    purpose text not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint database_restore_jobs_target_name_not_blank check (length(trim(target_managed_database_name)) > 0),
    constraint database_restore_jobs_target_engine_check check (target_managed_database_engine = 'postgres'),
    constraint database_restore_jobs_target_port_check check (target_managed_database_port between 1 and 65535),
    constraint database_restore_jobs_target_database_not_blank check (length(trim(target_managed_database_database)) > 0),
    constraint database_restore_jobs_target_username_not_blank check (length(trim(target_managed_database_username)) > 0),
    constraint database_restore_jobs_target_ssl_mode_check check (
        target_managed_database_ssl_mode in ('disable', 'prefer', 'require')
    ),
    constraint database_restore_jobs_format_check check (format = 'postgres_custom'),
    constraint database_restore_jobs_status_check check (
        status in ('queued', 'running', 'succeeded', 'failed', 'deleted')
    ),
    constraint database_restore_jobs_phase_not_blank check (length(trim(phase)) > 0),
    constraint database_restore_jobs_progress_percent_check check (progress_percent between 0 and 100),
    constraint database_restore_jobs_purpose_not_blank check (length(trim(purpose)) > 0)
);

create index if not exists database_restore_jobs_owner_created_at_idx
    on database_restore_jobs (owner_user_id, created_at desc);

create index if not exists database_restore_jobs_owner_backup_created_at_idx
    on database_restore_jobs (owner_user_id, backup_id, created_at desc);

create index if not exists database_restore_jobs_owner_target_created_at_idx
    on database_restore_jobs (owner_user_id, target_managed_database_id, created_at desc);

create index if not exists database_restore_jobs_status_created_at_idx
    on database_restore_jobs (status, created_at);
