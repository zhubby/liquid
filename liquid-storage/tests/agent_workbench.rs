use liquid_core::{
    AgentActionKind, AgentActionStatus, AgentEventType, AgentMessageRole, AgentResourceKind,
    AgentTurnStatus, CreateAgentActionRequest, CreateAgentConversationRequest,
    CreateAgentTurnRequest, RegisterRequest,
};
use liquid_storage::{LiquidStore, Storage, StorageOptions};
use serde_json::json;

#[tokio::test]
async fn agent_workbench_store_persists_turn_events_and_actions() {
    let Some(storage) = test_storage().await else {
        return;
    };

    let auth = storage
        .register_user(RegisterRequest {
            email: unique_email("agent-workbench"),
            display_name: "Agent Workbench Test".to_owned(),
            password: "password123".to_owned(),
        })
        .await
        .unwrap();
    let conversation = storage
        .create_agent_conversation(
            &auth.user.id,
            CreateAgentConversationRequest {
                title: Some("SQL agent".to_owned()),
            },
        )
        .await
        .unwrap();
    let turn = storage
        .create_agent_turn(
            &auth.user.id,
            &conversation.id,
            CreateAgentTurnRequest {
                message: "select * from users".to_owned(),
                managed_database_id: None,
                dashboard_context: None,
                client_request_id: Some("client-1".to_owned()),
            },
        )
        .await
        .unwrap();

    assert_eq!(turn.status, AgentTurnStatus::Queued);

    let event = storage
        .append_agent_turn_event(
            &auth.user.id,
            &turn.id,
            AgentEventType::TurnStarted,
            json!({ "turn_id": turn.id }),
        )
        .await
        .unwrap();
    assert_eq!(event.seq, 1);

    let events = storage
        .list_agent_turn_events(&auth.user.id, &turn.id, 0)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, AgentEventType::TurnStarted);

    let assistant = storage
        .append_agent_message(
            &auth.user.id,
            &conversation.id,
            Some(&turn.id),
            AgentMessageRole::Assistant,
            "Prepared an audit action.",
            None,
        )
        .await
        .unwrap();
    let completed = storage
        .set_agent_turn_assistant_message(&auth.user.id, &turn.id, &assistant.id)
        .await
        .unwrap();
    assert_eq!(
        completed.assistant_message_id.as_deref(),
        Some(assistant.id.as_str())
    );

    let action = storage
        .create_agent_action(
            &auth.user.id,
            &turn.id,
            CreateAgentActionRequest {
                kind: AgentActionKind::CreateSqlAudit,
                title: "Create SQL audit".to_owned(),
                description: "Create a SQL audit record.".to_owned(),
                payload: json!({ "sql": "select * from users" }),
                resource_kind: Some(AgentResourceKind::SqlAudit),
                resource_id: None,
                requires_confirmation: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(action.status, AgentActionStatus::Proposed);

    let listed = storage
        .list_agent_actions(
            &auth.user.id,
            Some(&conversation.id),
            Some(AgentActionStatus::Proposed),
        )
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
}

async fn test_storage() -> Option<Storage> {
    let database_url = std::env::var("LIQUID_TEST_DATABASE_URL").ok()?;
    let storage = Storage::connect_with_options(StorageOptions::new(database_url))
        .await
        .ok()?;
    storage.migrate().await.ok()?;
    Some(storage)
}

fn unique_email(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    format!("{prefix}-{nanos}@test.local")
}
