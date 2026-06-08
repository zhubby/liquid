alter table user_llm_provider_settings
    add column if not exists streaming_enabled boolean not null default true;
