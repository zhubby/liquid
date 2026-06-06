create table if not exists user_llm_provider_settings (
    owner_user_id uuid primary key references users(id) on delete cascade,
    provider text not null,
    base_url text not null,
    model text not null,
    api_mode text not null,
    encrypted_api_key text not null default '',
    updated_at timestamptz not null default now(),
    constraint user_llm_provider_settings_provider_check
        check (provider in ('openai_compatible')),
    constraint user_llm_provider_settings_api_mode_check
        check (api_mode in ('chat_completions', 'responses'))
);
