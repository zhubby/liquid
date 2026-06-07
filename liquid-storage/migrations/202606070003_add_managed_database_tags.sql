alter table managed_databases
    add column if not exists tags text[] not null default '{}'::text[];
