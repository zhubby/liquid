create table if not exists database_diagrams (
    id uuid primary key default gen_random_uuid(),
    owner_user_id uuid not null references users(id) on delete cascade,
    managed_database_id uuid not null references managed_databases(id) on delete cascade,
    title text not null,
    description text,
    document jsonb not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint database_diagrams_title_not_blank check (length(trim(title)) > 0),
    constraint database_diagrams_description_not_blank check (
        description is null or length(trim(description)) > 0
    ),
    constraint database_diagrams_document_object_check check (
        jsonb_typeof(document) = 'object'
    )
);

create index if not exists database_diagrams_owner_database_updated_at_idx
    on database_diagrams (owner_user_id, managed_database_id, updated_at desc);

create index if not exists database_diagrams_owner_updated_at_idx
    on database_diagrams (owner_user_id, updated_at desc);
