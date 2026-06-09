use std::{str::FromStr, sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use croner::Cron;
use liquid_core::{
    DatabaseBackupMetadataStore, DatabaseBackupMetadataStoreError, DatabaseBackupScheduleRecord,
    DatabaseBackupTrigger, EnqueueDatabaseBackup,
};
use time::OffsetDateTime;
use tokio::task::JoinHandle;

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(30);
const DEFAULT_MINIMUM_INTERVAL_SECONDS: i64 = 15 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseBackupSchedulerConfig {
    pub scheduler_id: String,
    pub poll_interval: Duration,
    pub minimum_interval_seconds: i64,
}

impl DatabaseBackupSchedulerConfig {
    pub fn new(scheduler_id: impl Into<String>) -> Self {
        Self {
            scheduler_id: scheduler_id.into(),
            poll_interval: DEFAULT_POLL_INTERVAL,
            minimum_interval_seconds: DEFAULT_MINIMUM_INTERVAL_SECONDS,
        }
    }
}

#[derive(Clone)]
pub struct DatabaseBackupScheduler {
    metadata_store: Arc<dyn DatabaseBackupMetadataStore>,
    config: DatabaseBackupSchedulerConfig,
}

impl DatabaseBackupScheduler {
    pub fn new(
        metadata_store: Arc<dyn DatabaseBackupMetadataStore>,
        config: DatabaseBackupSchedulerConfig,
    ) -> Self {
        Self {
            metadata_store,
            config,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            self.run_forever().await;
        })
    }

    pub async fn run_forever(self) {
        loop {
            match self.run_once(OffsetDateTime::now_utc()).await {
                Ok(true) => {}
                Ok(false) => tokio::time::sleep(self.config.poll_interval).await,
                Err(error) => {
                    tracing::error!(error = %error, "database backup scheduler iteration failed");
                    tokio::time::sleep(self.config.poll_interval).await;
                }
            }
        }
    }

    pub async fn run_once(&self, now: OffsetDateTime) -> Result<bool> {
        let Some(schedule) = self
            .metadata_store
            .claim_due_database_backup_schedule(&self.config.scheduler_id, now)
            .await?
        else {
            return Ok(false);
        };

        self.enqueue_schedule(schedule, now).await?;
        Ok(true)
    }

    async fn enqueue_schedule(
        &self,
        schedule: DatabaseBackupScheduleRecord,
        now: OffsetDateTime,
    ) -> Result<()> {
        let scheduled_for = schedule.next_run_at;
        let next_run_at = next_backup_run_at(&schedule.cron_expression, &schedule.timezone, now)?;
        let request = EnqueueDatabaseBackup {
            managed_database_id: schedule.source.id.clone(),
            purpose: schedule.purpose.clone(),
            schedule_id: Some(schedule.id.clone()),
            trigger: DatabaseBackupTrigger::Cron,
            scheduled_for: Some(scheduled_for),
            conversation_id: schedule.conversation_id.clone(),
            created_from_turn_id: schedule.created_from_turn_id.clone(),
        };

        match self
            .metadata_store
            .enqueue_database_backup(&schedule.owner_user_id, request)
            .await
        {
            Ok(_) => {}
            Err(DatabaseBackupMetadataStoreError::Conflict(message)) => {
                tracing::warn!(
                    schedule_id = %schedule.id,
                    scheduled_for = %scheduled_for,
                    error = %message,
                    "database backup schedule enqueue conflicted; advancing schedule"
                );
            }
            Err(error) => return Err(error.into()),
        }

        self.metadata_store
            .complete_database_backup_schedule_enqueue(
                &schedule.owner_user_id,
                &schedule.id,
                scheduled_for,
                next_run_at,
            )
            .await?;

        Ok(())
    }
}

pub fn validate_backup_schedule(
    cron_expression: &str,
    timezone: &str,
    now: OffsetDateTime,
    minimum_interval_seconds: i64,
) -> Result<OffsetDateTime> {
    let first = next_backup_run_at(cron_expression, timezone, now)?;
    let second = next_backup_run_at(cron_expression, timezone, first)?;
    let interval = second.unix_timestamp() - first.unix_timestamp();
    if interval < minimum_interval_seconds {
        anyhow::bail!(
            "database backup cron interval must be at least {} seconds",
            minimum_interval_seconds
        );
    }

    Ok(first)
}

pub fn next_backup_run_at(
    cron_expression: &str,
    timezone: &str,
    after: OffsetDateTime,
) -> Result<OffsetDateTime> {
    let cron = Cron::from_str(cron_expression)
        .with_context(|| format!("invalid database backup cron expression: {cron_expression}"))?;
    let tz = timezone
        .parse::<Tz>()
        .with_context(|| format!("invalid database backup timezone: {timezone}"))?;
    let after = offset_to_chrono_utc(after)?.with_timezone(&tz);
    let next = cron
        .find_next_occurrence(&after, false)
        .map_err(|error| anyhow!("failed to calculate next database backup run: {error}"))?;

    chrono_utc_to_offset(next.with_timezone(&Utc))
}

fn offset_to_chrono_utc(value: OffsetDateTime) -> Result<DateTime<Utc>> {
    Utc.timestamp_opt(value.unix_timestamp(), value.nanosecond())
        .single()
        .ok_or_else(|| anyhow!("invalid UTC timestamp"))
}

fn chrono_utc_to_offset(value: DateTime<Utc>) -> Result<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp(value.timestamp())
        .context("invalid next database backup timestamp")
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use liquid_core::{
        CompleteDatabaseBackup, DatabaseBackupFormat, DatabaseBackupRecord,
        DatabaseBackupScheduleStatus, DatabaseBackupStatus, DatabaseRestoreRecord,
        ManagedDatabaseEngine, ManagedDatabaseSnapshot, ManagedDatabaseSslMode,
    };
    use time::format_description::well_known::Rfc3339;

    use super::*;

    #[test]
    fn next_backup_run_at_uses_schedule_timezone() {
        let next =
            next_backup_run_at("0 9 * * *", "Asia/Shanghai", utc("2026-06-09T00:00:00Z")).unwrap();

        assert_eq!(next, utc("2026-06-09T01:00:00Z"));
    }

    #[test]
    fn validate_backup_schedule_rejects_intervals_below_minimum() {
        let error =
            validate_backup_schedule("*/5 * * * *", "UTC", utc("2026-06-09T00:00:00Z"), 15 * 60)
                .unwrap_err();

        assert!(error.to_string().contains("at least 900 seconds"));
    }

    #[tokio::test]
    async fn scheduler_enqueues_one_job_for_missed_cron_and_advances_to_next_future_run() {
        let scheduled_for = utc("2026-06-09T00:00:00Z");
        let now = utc("2026-06-09T02:37:00Z");
        let store = Arc::new(SchedulerStore::new(Some(schedule(scheduled_for))));
        let scheduler = DatabaseBackupScheduler::new(
            store.clone(),
            DatabaseBackupSchedulerConfig::new("scheduler-1"),
        );

        assert!(scheduler.run_once(now).await.unwrap());
        assert!(!scheduler.run_once(now).await.unwrap());

        let enqueued = store.enqueued.lock().unwrap();
        assert_eq!(enqueued.len(), 1);
        let (owner_user_id, request) = &enqueued[0];
        assert_eq!(owner_user_id, "user-1");
        assert_eq!(request.managed_database_id, "db-1");
        assert_eq!(request.schedule_id.as_deref(), Some("schedule-1"));
        assert_eq!(request.trigger, DatabaseBackupTrigger::Cron);
        assert_eq!(request.scheduled_for, Some(scheduled_for));
        assert_eq!(request.conversation_id.as_deref(), Some("conversation-1"));
        assert_eq!(request.created_from_turn_id.as_deref(), Some("turn-1"));

        let completed = store.completed.lock().unwrap();
        assert_eq!(
            completed.as_slice(),
            &[(
                "user-1".to_owned(),
                "schedule-1".to_owned(),
                scheduled_for,
                utc("2026-06-09T03:00:00Z"),
            )]
        );
    }

    struct SchedulerStore {
        due: Mutex<Option<DatabaseBackupScheduleRecord>>,
        enqueued: Mutex<Vec<(String, EnqueueDatabaseBackup)>>,
        completed: Mutex<Vec<(String, String, OffsetDateTime, OffsetDateTime)>>,
    }

    impl SchedulerStore {
        fn new(due: Option<DatabaseBackupScheduleRecord>) -> Self {
            Self {
                due: Mutex::new(due),
                enqueued: Mutex::new(Vec::new()),
                completed: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl DatabaseBackupMetadataStore for SchedulerStore {
        async fn create_database_backup(
            &self,
            _owner_user_id: &str,
            _source_managed_database_id: &str,
            _purpose: Option<String>,
        ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn enqueue_database_backup(
            &self,
            owner_user_id: &str,
            request: EnqueueDatabaseBackup,
        ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
            self.enqueued
                .lock()
                .unwrap()
                .push((owner_user_id.to_owned(), request.clone()));

            Ok(backup_record(owner_user_id, &request))
        }

        async fn get_database_backup(
            &self,
            _owner_user_id: &str,
            _id: &str,
        ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn list_database_backups(
            &self,
            _owner_user_id: &str,
            _source_managed_database_id: Option<&str>,
            _status: Option<DatabaseBackupStatus>,
            _limit: i64,
        ) -> Result<Vec<DatabaseBackupRecord>, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn delete_database_backup(
            &self,
            _owner_user_id: &str,
            _id: &str,
        ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn create_database_restore(
            &self,
            _owner_user_id: &str,
            _backup_id: &str,
            _target_managed_database_id: &str,
            _purpose: String,
        ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn get_database_restore(
            &self,
            _owner_user_id: &str,
            _id: &str,
        ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn list_database_restores(
            &self,
            _owner_user_id: &str,
            _backup_id: Option<&str>,
            _target_managed_database_id: Option<&str>,
            _status: Option<DatabaseBackupStatus>,
            _limit: i64,
        ) -> Result<Vec<DatabaseRestoreRecord>, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn claim_next_database_backup(
            &self,
            _worker_id: &str,
        ) -> Result<Option<DatabaseBackupRecord>, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn update_database_backup_progress(
            &self,
            _id: &str,
            _phase: &str,
            _progress_percent: i32,
        ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn complete_database_backup(
            &self,
            _id: &str,
            _result: CompleteDatabaseBackup,
        ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn fail_database_backup(
            &self,
            _id: &str,
            _error: String,
        ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn claim_next_database_restore(
            &self,
            _worker_id: &str,
        ) -> Result<Option<DatabaseRestoreRecord>, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn update_database_restore_progress(
            &self,
            _id: &str,
            _phase: &str,
            _progress_percent: i32,
        ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn complete_database_restore(
            &self,
            _id: &str,
        ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn fail_database_restore(
            &self,
            _id: &str,
            _error: String,
        ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn fail_stale_database_jobs(
            &self,
            _stale_after_seconds: i64,
        ) -> Result<u64, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn claim_due_database_backup_schedule(
            &self,
            scheduler_id: &str,
            _now: OffsetDateTime,
        ) -> Result<Option<DatabaseBackupScheduleRecord>, DatabaseBackupMetadataStoreError>
        {
            assert_eq!(scheduler_id, "scheduler-1");

            Ok(self.due.lock().unwrap().take())
        }

        async fn complete_database_backup_schedule_enqueue(
            &self,
            owner_user_id: &str,
            id: &str,
            scheduled_for: OffsetDateTime,
            next_run_at: OffsetDateTime,
        ) -> Result<DatabaseBackupScheduleRecord, DatabaseBackupMetadataStoreError> {
            self.completed.lock().unwrap().push((
                owner_user_id.to_owned(),
                id.to_owned(),
                scheduled_for,
                next_run_at,
            ));

            let mut schedule = schedule(scheduled_for);
            schedule.last_enqueued_at = Some(scheduled_for);
            schedule.next_run_at = next_run_at;
            Ok(schedule)
        }
    }

    fn schedule(next_run_at: OffsetDateTime) -> DatabaseBackupScheduleRecord {
        DatabaseBackupScheduleRecord {
            id: "schedule-1".to_owned(),
            owner_user_id: "user-1".to_owned(),
            source: snapshot(),
            cron_expression: "0 * * * *".to_owned(),
            timezone: "UTC".to_owned(),
            status: DatabaseBackupScheduleStatus::Active,
            purpose: Some("nightly backup".to_owned()),
            keep_last: Some(3),
            retention_days: None,
            conversation_id: Some("conversation-1".to_owned()),
            created_from_turn_id: Some("turn-1".to_owned()),
            last_enqueued_at: None,
            next_run_at,
            created_at: utc("2026-06-08T00:00:00Z"),
            updated_at: utc("2026-06-08T00:00:00Z"),
        }
    }

    fn backup_record(owner_user_id: &str, request: &EnqueueDatabaseBackup) -> DatabaseBackupRecord {
        DatabaseBackupRecord {
            id: "backup-1".to_owned(),
            owner_user_id: owner_user_id.to_owned(),
            source: snapshot(),
            format: DatabaseBackupFormat::PostgresCustom,
            status: DatabaseBackupStatus::Queued,
            phase: "queued".to_owned(),
            progress_percent: 0,
            schedule_id: request.schedule_id.clone(),
            trigger: request.trigger,
            scheduled_for: request.scheduled_for,
            conversation_id: request.conversation_id.clone(),
            created_from_turn_id: request.created_from_turn_id.clone(),
            storage: None,
            postgres_server_version: None,
            pg_dump_version: None,
            error: None,
            purpose: request.purpose.clone(),
            worker_id: None,
            heartbeat_at: None,
            started_at: None,
            completed_at: None,
            created_at: utc("2026-06-09T00:00:00Z"),
            updated_at: utc("2026-06-09T00:00:00Z"),
        }
    }

    fn snapshot() -> ManagedDatabaseSnapshot {
        ManagedDatabaseSnapshot {
            id: "db-1".to_owned(),
            name: "Warehouse".to_owned(),
            engine: ManagedDatabaseEngine::Postgres,
            host: "localhost".to_owned(),
            port: 5432,
            database: "warehouse".to_owned(),
            username: "postgres".to_owned(),
            ssl_mode: ManagedDatabaseSslMode::Disable,
        }
    }

    fn utc(value: &str) -> OffsetDateTime {
        OffsetDateTime::parse(value, &Rfc3339).unwrap()
    }
}
