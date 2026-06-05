create table if not exists user_managed_database_preferences (
    owner_user_id uuid primary key references users(id) on delete cascade,
    current_managed_database_id uuid references managed_databases(id) on delete set null,
    updated_at timestamptz not null default now()
);
