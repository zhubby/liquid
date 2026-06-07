alter table agent_actions
    drop constraint if exists agent_actions_kind_check;

alter table agent_actions
    drop constraint if exists agent_actions_resource_kind_check;

update agent_actions
set kind = 'create_datapanel_card'
where kind = 'create_bi_card';

update agent_actions
set resource_kind = 'datapanel_card'
where resource_kind = 'bi_panel_card';

alter table if exists bi_panels
    rename to datapanels;

alter table if exists bi_panel_cards
    rename to datapanel_cards;

alter table datapanels
    rename constraint bi_panels_pkey to datapanels_pkey;

alter table datapanels
    rename constraint bi_panels_conversation_id_fkey to datapanels_conversation_id_fkey;

alter table datapanels
    rename constraint bi_panels_owner_user_id_fkey to datapanels_owner_user_id_fkey;

alter table datapanels
    rename constraint bi_panels_title_not_blank to datapanels_title_not_blank;

alter table datapanels
    rename constraint bi_panels_description_not_blank to datapanels_description_not_blank;

alter table datapanel_cards
    rename constraint bi_panel_cards_pkey to datapanel_cards_pkey;

alter table datapanel_cards
    rename constraint bi_panel_cards_panel_id_fkey to datapanel_cards_panel_id_fkey;

alter table datapanel_cards
    rename constraint bi_panel_cards_owner_user_id_fkey to datapanel_cards_owner_user_id_fkey;

alter table datapanel_cards
    rename constraint bi_panel_cards_managed_database_id_fkey to datapanel_cards_managed_database_id_fkey;

alter table datapanel_cards
    rename constraint bi_panel_cards_source_action_id_fkey to datapanel_cards_source_action_id_fkey;

alter table datapanel_cards
    rename constraint bi_panel_cards_kind_check to datapanel_cards_kind_check;

alter table datapanel_cards
    rename constraint bi_panel_cards_title_not_blank to datapanel_cards_title_not_blank;

alter table datapanel_cards
    rename constraint bi_panel_cards_description_not_blank to datapanel_cards_description_not_blank;

alter table datapanel_cards
    rename constraint bi_panel_cards_sql_not_blank to datapanel_cards_sql_not_blank;

alter index if exists bi_panels_conversation_unique_idx
    rename to datapanels_conversation_unique_idx;

alter index if exists bi_panels_owner_updated_at_idx
    rename to datapanels_owner_updated_at_idx;

alter index if exists bi_panel_cards_panel_updated_at_idx
    rename to datapanel_cards_panel_updated_at_idx;

alter table agent_actions
    add constraint agent_actions_kind_check check (
        kind in (
            'create_sql_audit',
            'create_datapanel_card',
            'approve_sql_audit',
            'reject_sql_audit',
            'execute_sql_audit',
            'create_managed_database',
            'update_managed_database',
            'delete_managed_database',
            'start_database_backup',
            'start_database_restore'
        )
    );

alter table agent_actions
    add constraint agent_actions_resource_kind_check check (
        resource_kind is null or resource_kind in (
            'sql_audit',
            'datapanel_card',
            'managed_database',
            'database_backup',
            'database_restore'
        )
    );
