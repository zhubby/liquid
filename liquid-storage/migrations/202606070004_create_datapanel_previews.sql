create table if not exists datapanel_previews (
    id uuid primary key default gen_random_uuid(),
    panel_id uuid not null references datapanels(id) on delete cascade,
    owner_user_id uuid not null references users(id) on delete cascade,
    slug text not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint datapanel_previews_slug_not_blank check (length(trim(slug)) > 0)
);

create unique index if not exists datapanel_previews_panel_unique_idx
    on datapanel_previews (panel_id);

create unique index if not exists datapanel_previews_slug_unique_idx
    on datapanel_previews (slug);
