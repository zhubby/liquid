alter table agent_conversations
    add column if not exists managed_database_id uuid references managed_databases(id) on delete cascade;

with latest_turn_database as (
    select distinct on (conversation_id)
        conversation_id,
        managed_database_id
    from agent_turns
    where managed_database_id is not null
    order by conversation_id, created_at desc, id desc
)
update agent_conversations
set managed_database_id = latest_turn_database.managed_database_id
from latest_turn_database
where agent_conversations.id = latest_turn_database.conversation_id
  and agent_conversations.managed_database_id is null;

create index if not exists agent_conversations_owner_database_updated_at_idx
    on agent_conversations (owner_user_id, managed_database_id, updated_at desc);
