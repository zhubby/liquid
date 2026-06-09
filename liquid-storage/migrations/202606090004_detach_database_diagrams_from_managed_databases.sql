drop index if exists database_diagrams_owner_database_updated_at_idx;

alter table if exists database_diagrams
    drop column if exists managed_database_id;

create index if not exists database_diagrams_owner_updated_at_idx
    on database_diagrams (owner_user_id, updated_at desc);
