create extension if not exists pgcrypto;

create table if not exists users (
    id uuid primary key default gen_random_uuid(),
    email text not null,
    display_name text not null,
    password_hash text not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint users_email_not_blank check (length(trim(email)) > 0),
    constraint users_display_name_not_blank check (length(trim(display_name)) > 0)
);

create unique index if not exists users_email_unique_idx on users (lower(email));

create table if not exists auth_tokens (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    token_hash text not null,
    expires_at timestamptz not null,
    revoked_at timestamptz,
    created_at timestamptz not null default now(),
    constraint auth_tokens_token_hash_not_blank check (length(trim(token_hash)) > 0)
);

create unique index if not exists auth_tokens_token_hash_unique_idx on auth_tokens (token_hash);
create index if not exists auth_tokens_active_lookup_idx on auth_tokens (token_hash, expires_at)
    where revoked_at is null;

create table if not exists audited_databases (
    id uuid primary key default gen_random_uuid(),
    owner_user_id uuid not null references users(id) on delete cascade,
    name text not null,
    engine text not null default 'postgres',
    host text not null,
    port integer not null,
    database_name text not null,
    username text not null,
    encrypted_password text not null,
    ssl_mode text not null default 'prefer',
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint audited_databases_engine_check check (engine = 'postgres'),
    constraint audited_databases_ssl_mode_check check (ssl_mode in ('disable', 'prefer', 'require')),
    constraint audited_databases_port_check check (port between 1 and 65535),
    constraint audited_databases_name_not_blank check (length(trim(name)) > 0),
    constraint audited_databases_host_not_blank check (length(trim(host)) > 0),
    constraint audited_databases_database_not_blank check (length(trim(database_name)) > 0),
    constraint audited_databases_username_not_blank check (length(trim(username)) > 0)
);

create unique index if not exists audited_databases_owner_name_unique_idx
    on audited_databases (owner_user_id, lower(name));
