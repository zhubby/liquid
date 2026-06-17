alter table agent_actions
    drop constraint if exists agent_actions_kind_check;

alter table agent_actions
    drop constraint if exists agent_actions_resource_kind_check;

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
            'start_database_restore',
            'create_database_diagram'
        )
    );

alter table agent_actions
    add constraint agent_actions_resource_kind_check check (
        resource_kind is null or resource_kind in (
            'sql_audit',
            'datapanel_card',
            'managed_database',
            'database_backup',
            'database_restore',
            'database_diagram'
        )
    );
