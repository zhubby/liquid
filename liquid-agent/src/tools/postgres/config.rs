use liquid_sql::PgSqlMetadataOptions;
use sqlx::PgPool;

pub(crate) const DEFAULT_TOOL_LIMIT: usize = 100;
pub(crate) const MAX_TOOL_LIMIT: usize = 1_000;
pub(crate) const MAX_SQL_OUTPUT_BYTES: usize = 256 * 1_024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PostgresToolExecutionMode {
    Off,
    #[default]
    Readonly,
    WriteGated,
}

#[derive(Debug, Clone)]
pub struct PostgresToolConfig {
    pub(crate) pool: Option<PgPool>,
    pub(crate) metadata_required: bool,
    pub(crate) execution: PostgresToolExecutionMode,
    metadata_options: PgSqlMetadataOptions,
    default_limit: usize,
    max_limit: usize,
    max_output_bytes: usize,
}

impl PostgresToolConfig {
    pub fn new(
        pool: Option<PgPool>,
        metadata_required: bool,
        execution: PostgresToolExecutionMode,
    ) -> Self {
        Self {
            pool,
            metadata_required,
            execution,
            metadata_options: PgSqlMetadataOptions::default(),
            default_limit: DEFAULT_TOOL_LIMIT,
            max_limit: MAX_TOOL_LIMIT,
            max_output_bytes: MAX_SQL_OUTPUT_BYTES,
        }
    }

    pub fn with_metadata_options(mut self, metadata_options: PgSqlMetadataOptions) -> Self {
        self.metadata_options = metadata_options;
        self
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PostgresToolContext {
    pub(crate) pool: PgPool,
    pub(crate) metadata_options: PgSqlMetadataOptions,
    pub(crate) default_limit: usize,
    pub(crate) max_limit: usize,
    pub(crate) max_output_bytes: usize,
}

impl PostgresToolContext {
    pub(crate) fn new(pool: PgPool, config: &PostgresToolConfig) -> Self {
        Self {
            pool,
            metadata_options: config.metadata_options.clone(),
            default_limit: config.default_limit,
            max_limit: config.max_limit,
            max_output_bytes: config.max_output_bytes,
        }
    }
}
