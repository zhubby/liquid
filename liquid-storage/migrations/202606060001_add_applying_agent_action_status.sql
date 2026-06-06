alter table agent_actions
    drop constraint if exists agent_actions_status_check;

alter table agent_actions
    add constraint agent_actions_status_check check (
        status in ('proposed', 'applying', 'applied', 'rejected', 'failed', 'superseded')
    );
