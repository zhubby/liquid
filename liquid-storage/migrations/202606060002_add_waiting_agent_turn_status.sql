alter table agent_turns
    drop constraint if exists agent_turns_status_check;

alter table agent_turns
    add constraint agent_turns_status_check check (
        status in (
            'queued',
            'running',
            'waiting_for_user',
            'completed',
            'blocked',
            'failed',
            'cancelled'
        )
    );

alter table agent_turn_events
    drop constraint if exists agent_turn_events_type_check;

alter table agent_turn_events
    add constraint agent_turn_events_type_check check (
        event_type in (
            'turn_started',
            'message_created',
            'assistant_delta',
            'tool_call_started',
            'tool_call_finished',
            'resource_created',
            'resource_updated',
            'action_proposed',
            'turn_waiting_for_user',
            'turn_completed',
            'turn_failed'
        )
    );
