create table if not exists database_operation_diagnostics (
    id uuid primary key default gen_random_uuid(),
    owner_user_id uuid not null references users(id) on delete cascade,
    operation_kind text not null,
    operation_id uuid not null,
    phase text not null,
    message text not null,
    command_name text,
    exit_code integer,
    stdout text,
    stderr text,
    stdout_truncated boolean not null default false,
    stderr_truncated boolean not null default false,
    created_at timestamptz not null default now(),
    constraint database_operation_diagnostics_kind_check check (
        operation_kind in ('backup', 'restore')
    ),
    constraint database_operation_diagnostics_phase_not_blank check (
        length(trim(phase)) > 0
    ),
    constraint database_operation_diagnostics_message_not_blank check (
        length(trim(message)) > 0
    ),
    constraint database_operation_diagnostics_command_name_not_blank check (
        command_name is null or length(trim(command_name)) > 0
    ),
    constraint database_operation_diagnostics_stdout_size_check check (
        stdout is null or octet_length(stdout) <= 65536
    ),
    constraint database_operation_diagnostics_stderr_size_check check (
        stderr is null or octet_length(stderr) <= 65536
    )
);

create index if not exists database_operation_diagnostics_operation_created_at_idx
    on database_operation_diagnostics (
        owner_user_id,
        operation_kind,
        operation_id,
        created_at desc
    );
