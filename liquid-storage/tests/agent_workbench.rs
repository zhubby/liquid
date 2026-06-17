use liquid_core::{
    AgentActionKind, AgentActionStatus, AgentEventType, AgentMessageRole, AgentResourceKind,
    AgentTurnStatus, CreateAgentActionRequest, CreateAgentConversationRequest,
    CreateAgentTurnRequest, CreateDatapanelCardRequest, CreateManagedDatabaseRequest,
    DatapanelCardKind, DatapanelCardLayout, DatapanelCardLayoutUpdate, DatapanelQueryResult,
    ManagedDatabaseEngine, ManagedDatabaseSslMode, RegisterRequest,
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
                managed_database_id: None,
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
    let second_action = storage
        .create_agent_action(
            &auth.user.id,
            &turn.id,
            CreateAgentActionRequest {
                kind: AgentActionKind::CreateSqlAudit,
                title: "Create follow-up SQL audit".to_owned(),
                description: "Create a follow-up SQL audit record.".to_owned(),
                payload: json!({ "sql": "insert into users (id) values (1)" }),
                resource_kind: Some(AgentResourceKind::SqlAudit),
                resource_id: None,
                requires_confirmation: true,
            },
        )
        .await
        .unwrap();

    let listed = storage
        .list_agent_actions(
            &auth.user.id,
            Some(&conversation.id),
            Some(AgentActionStatus::Proposed),
        )
        .await
        .unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>(),
        vec![action.id.as_str(), second_action.id.as_str()]
    );
}

#[tokio::test]
async fn agent_workbench_get_or_create_conversation_is_idempotent() {
    let Some(storage) = test_storage().await else {
        return;
    };

    let auth = storage
        .register_user(RegisterRequest {
            email: unique_email("agent-workbench-default"),
            display_name: "Agent Workbench Default Test".to_owned(),
            password: "password123".to_owned(),
        })
        .await
        .unwrap();
    let database = storage
        .create_managed_database(
            &auth.user.id,
            CreateManagedDatabaseRequest {
                name: "Warehouse".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "app".to_owned(),
                username: "postgres".to_owned(),
                password: "password123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();
    let legacy_conversation = storage
        .create_agent_conversation(
            &auth.user.id,
            CreateAgentConversationRequest {
                title: Some("Existing workspace".to_owned()),
                managed_database_id: None,
            },
        )
        .await
        .unwrap();

    let rebound_conversation = storage
        .get_or_create_agent_conversation(
            &auth.user.id,
            CreateAgentConversationRequest {
                title: Some("Default workspace".to_owned()),
                managed_database_id: Some(database.id.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(rebound_conversation.id, legacy_conversation.id);
    assert_eq!(rebound_conversation.title, "Existing workspace");
    assert_eq!(
        rebound_conversation.managed_database_id.as_deref(),
        Some(database.id.as_str())
    );

    let same_conversation = storage
        .get_or_create_agent_conversation(
            &auth.user.id,
            CreateAgentConversationRequest {
                title: Some("Should not overwrite".to_owned()),
                managed_database_id: Some(database.id.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(same_conversation.id, rebound_conversation.id);
    assert_eq!(same_conversation.title, "Existing workspace");

    let second_database = storage
        .create_managed_database(
            &auth.user.id,
            CreateManagedDatabaseRequest {
                name: "Doro".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "doro".to_owned(),
                username: "postgres".to_owned(),
                password: "password123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();
    let created_conversation = storage
        .get_or_create_agent_conversation(
            &auth.user.id,
            CreateAgentConversationRequest {
                title: Some("Doro workspace".to_owned()),
                managed_database_id: Some(second_database.id.clone()),
            },
        )
        .await
        .unwrap();
    let created_again = storage
        .get_or_create_agent_conversation(
            &auth.user.id,
            CreateAgentConversationRequest {
                title: Some("Another Doro workspace".to_owned()),
                managed_database_id: Some(second_database.id.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(created_again.id, created_conversation.id);
    assert_eq!(created_again.title, "Doro workspace");
}

#[tokio::test]
async fn datapanel_store_persists_cards_layouts_and_export() {
    let Some(storage) = test_storage().await else {
        return;
    };

    let auth = storage
        .register_user(RegisterRequest {
            email: unique_email("datapanel"),
            display_name: "Datapanel Test".to_owned(),
            password: "password123".to_owned(),
        })
        .await
        .unwrap();
    let database = storage
        .create_managed_database(
            &auth.user.id,
            CreateManagedDatabaseRequest {
                name: "Warehouse".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "app".to_owned(),
                username: "postgres".to_owned(),
                password: "password123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();
    let conversation = storage
        .create_agent_conversation(
            &auth.user.id,
            CreateAgentConversationRequest {
                title: Some("Sales analysis".to_owned()),
                managed_database_id: Some(database.id.clone()),
            },
        )
        .await
        .unwrap();

    let panel = storage
        .get_or_create_datapanel(&auth.user.id, &conversation.id)
        .await
        .unwrap();
    let same_panel = storage
        .get_or_create_datapanel(&auth.user.id, &conversation.id)
        .await
        .unwrap();
    assert_eq!(panel.id, same_panel.id);
    assert_eq!(
        panel.description.as_deref(),
        Some("用于沉淀当前会话的数据查询结果与图表")
    );

    let card = storage
        .create_datapanel_card(
            &auth.user.id,
            &panel.id,
            CreateDatapanelCardRequest {
                managed_database_id: database.id,
                source_action_id: None,
                title: "Daily revenue".to_owned(),
                description: Some("Revenue by day".to_owned()),
                kind: DatapanelCardKind::Table,
                sql: "select '2026-06-06' as day, 42 as revenue".to_owned(),
                chart: None,
                layout: DatapanelCardLayout {
                    x: 0,
                    y: 0,
                    w: 6,
                    h: 4,
                },
                result: DatapanelQueryResult {
                    columns: vec!["day".to_owned(), "revenue".to_owned()],
                    rows: vec![json!({ "day": "2026-06-06", "revenue": 42 })],
                    row_count: 1,
                    truncated: false,
                    elapsed_ms: 1,
                    refreshed_at: time::OffsetDateTime::UNIX_EPOCH,
                },
            },
        )
        .await
        .unwrap();

    let updated = storage
        .update_datapanel_layout(
            &auth.user.id,
            &panel.id,
            vec![DatapanelCardLayoutUpdate {
                card_id: card.id.clone(),
                layout: DatapanelCardLayout {
                    x: 6,
                    y: 1,
                    w: 6,
                    h: 5,
                },
            }],
        )
        .await
        .unwrap();

    assert_eq!(updated.cards.len(), 1);
    assert_eq!(updated.cards[0].layout.x, 6);
    assert_eq!(updated.cards[0].layout.h, 5);

    let export = storage
        .export_datapanel(&auth.user.id, &panel.id)
        .await
        .unwrap();
    assert_eq!(export.panel.cards.len(), 1);
    assert_eq!(export.panel.cards[0].title, "Daily revenue");
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
