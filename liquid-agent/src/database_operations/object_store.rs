use std::{env, path::Path};

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use reqwest::{Method, StatusCode};
use ring::hmac;
use sha2::{Digest, Sha256};
use time::{Month, OffsetDateTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectStoreWriteResult {
    pub bucket: String,
    pub key: String,
    pub version_id: Option<String>,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectStoreReadResult {
    pub size_bytes: i64,
    pub etag: Option<String>,
}

#[async_trait]
pub trait BackupObjectStore: Send + Sync {
    fn bucket(&self) -> &str;

    async fn put_object(&self, key: &str, file_path: &Path) -> Result<ObjectStoreWriteResult>;

    async fn get_object(&self, key: &str, file_path: &Path) -> Result<ObjectStoreReadResult>;

    async fn delete_object(&self, key: &str) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3BackupObjectStoreConfig {
    pub bucket: String,
    pub region: String,
    pub prefix: String,
    pub endpoint: Option<String>,
    pub path_style: bool,
}

impl S3BackupObjectStoreConfig {
    pub fn new(bucket: impl Into<String>, region: impl Into<String>) -> Self {
        Self {
            bucket: bucket.into(),
            region: region.into(),
            prefix: String::new(),
            endpoint: None,
            path_style: false,
        }
    }

    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    pub fn with_endpoint(mut self, endpoint: Option<String>) -> Self {
        self.endpoint = endpoint;
        self
    }

    pub fn with_path_style(mut self, path_style: bool) -> Self {
        self.path_style = path_style;
        self
    }

    pub fn object_key(&self, owner_user_id: &str, database_id: &str, backup_id: &str) -> String {
        let mut parts = Vec::new();
        let prefix = self.prefix.trim_matches('/');
        if !prefix.is_empty() {
            parts.push(prefix.to_owned());
        }
        parts.push(owner_user_id.to_owned());
        parts.push(database_id.to_owned());
        parts.push(format!("{backup_id}.dump"));
        parts.join("/")
    }
}

#[derive(Clone)]
pub struct S3BackupObjectStore {
    config: S3BackupObjectStoreConfig,
    credentials: AwsCredentials,
    client: reqwest::Client,
}

impl std::fmt::Debug for S3BackupObjectStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("S3BackupObjectStore")
            .field("bucket", &self.config.bucket)
            .field("region", &self.config.region)
            .field("prefix", &self.config.prefix)
            .field("endpoint", &self.config.endpoint)
            .field("path_style", &self.config.path_style)
            .finish_non_exhaustive()
    }
}

impl S3BackupObjectStore {
    pub fn from_env(config: S3BackupObjectStoreConfig) -> Result<Self> {
        let credentials = AwsCredentials::from_env()?;

        Ok(Self {
            config,
            credentials,
            client: reqwest::Client::new(),
        })
    }

    fn signed_request(&self, method: Method, key: &str) -> Result<SignedS3Request> {
        let target = S3Target::new(&self.config, key)?;
        let timestamp = AwsTimestamp::now();
        let credential_scope = format!(
            "{}/{}/s3/aws4_request",
            timestamp.short_date, self.config.region
        );
        let payload_hash = "UNSIGNED-PAYLOAD";

        let mut canonical_headers = vec![
            ("host", target.host_header.as_str()),
            ("x-amz-content-sha256", payload_hash),
            ("x-amz-date", timestamp.amz_date.as_str()),
        ];
        if let Some(token) = self.credentials.session_token.as_deref() {
            canonical_headers.push(("x-amz-security-token", token));
        }
        canonical_headers.sort_by_key(|(name, _)| *name);

        let signed_headers = canonical_headers
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(";");
        let canonical_headers = canonical_headers
            .iter()
            .map(|(name, value)| format!("{name}:{value}\n"))
            .collect::<String>();
        let canonical_request = format!(
            "{}\n{}\n\n{}{}\n{}",
            method.as_str(),
            target.canonical_uri,
            canonical_headers,
            signed_headers,
            payload_hash
        );
        let canonical_request_hash = sha256_hex(canonical_request.as_bytes());
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            timestamp.amz_date, credential_scope, canonical_request_hash
        );
        let signing_key = signing_key(
            &self.credentials.secret_access_key,
            &timestamp.short_date,
            &self.config.region,
        );
        let signature = hex::encode(hmac_sha256(&signing_key, string_to_sign.as_bytes()));
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.credentials.access_key_id, credential_scope, signed_headers, signature
        );

        Ok(SignedS3Request {
            url: target.url,
            host_header: target.host_header,
            amz_date: timestamp.amz_date,
            authorization,
            session_token: self.credentials.session_token.clone(),
        })
    }
}

#[async_trait]
impl BackupObjectStore for S3BackupObjectStore {
    fn bucket(&self) -> &str {
        &self.config.bucket
    }

    async fn put_object(&self, key: &str, file_path: &Path) -> Result<ObjectStoreWriteResult> {
        let request = self.signed_request(Method::PUT, key)?;
        let bytes = tokio::fs::read(file_path)
            .await
            .with_context(|| format!("failed to read backup file: {}", file_path.display()))?;
        let response = request
            .apply(self.client.request(Method::PUT, request.url.clone()))
            .body(bytes)
            .send()
            .await?;
        let status = response.status();
        let version_id = response_header(&response, "x-amz-version-id");
        let etag = response_header(&response, "etag");
        let body = response.text().await?;

        ensure_success(status, "S3 PUT object", body).await?;

        Ok(ObjectStoreWriteResult {
            bucket: self.config.bucket.clone(),
            key: key.to_owned(),
            version_id,
            etag,
        })
    }

    async fn get_object(&self, key: &str, file_path: &Path) -> Result<ObjectStoreReadResult> {
        let request = self.signed_request(Method::GET, key)?;
        let response = request
            .apply(self.client.request(Method::GET, request.url.clone()))
            .send()
            .await?;
        let status = response.status();
        let etag = response_header(&response, "etag");
        let bytes = response.bytes().await?;

        ensure_success(
            status,
            "S3 GET object",
            String::from_utf8_lossy(&bytes).into(),
        )
        .await?;

        tokio::fs::write(file_path, &bytes)
            .await
            .with_context(|| format!("failed to write backup file: {}", file_path.display()))?;

        Ok(ObjectStoreReadResult {
            size_bytes: i64::try_from(bytes.len()).unwrap_or(i64::MAX),
            etag,
        })
    }

    async fn delete_object(&self, key: &str) -> Result<()> {
        let request = self.signed_request(Method::DELETE, key)?;
        let response = request
            .apply(self.client.request(Method::DELETE, request.url.clone()))
            .send()
            .await?;

        ensure_success(
            response.status(),
            "S3 DELETE object",
            response.text().await?,
        )
        .await
    }
}

#[derive(Clone)]
struct AwsCredentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
}

impl AwsCredentials {
    fn from_env() -> Result<Self> {
        let access_key_id = env::var("AWS_ACCESS_KEY_ID")
            .or_else(|_| env::var("AWS_ACCESS_KEY"))
            .context("AWS_ACCESS_KEY_ID is required for S3 database backups")?;
        let secret_access_key = env::var("AWS_SECRET_ACCESS_KEY")
            .or_else(|_| env::var("AWS_SECRET_KEY"))
            .context("AWS_SECRET_ACCESS_KEY is required for S3 database backups")?;
        let session_token = env::var("AWS_SESSION_TOKEN")
            .ok()
            .filter(|value| !value.is_empty());

        Ok(Self {
            access_key_id,
            secret_access_key,
            session_token,
        })
    }
}

struct SignedS3Request {
    url: String,
    host_header: String,
    amz_date: String,
    authorization: String,
    session_token: Option<String>,
}

impl SignedS3Request {
    fn apply(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut builder = builder
            .header("host", &self.host_header)
            .header("x-amz-date", &self.amz_date)
            .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
            .header("authorization", &self.authorization);

        if let Some(token) = self.session_token.as_deref() {
            builder = builder.header("x-amz-security-token", token);
        }

        builder
    }
}

struct S3Target {
    url: String,
    host_header: String,
    canonical_uri: String,
}

impl S3Target {
    fn new(config: &S3BackupObjectStoreConfig, key: &str) -> Result<Self> {
        let endpoint = config
            .endpoint
            .clone()
            .unwrap_or_else(|| format!("https://s3.{}.amazonaws.com", config.region));
        let endpoint = reqwest::Url::parse(endpoint.trim_end_matches('/'))
            .with_context(|| format!("invalid S3 endpoint: {endpoint}"))?;
        let scheme = endpoint.scheme();
        let base_host = endpoint
            .host_str()
            .ok_or_else(|| anyhow!("S3 endpoint must include a host"))?;
        let base_host = match endpoint.port() {
            Some(port) => format!("{base_host}:{port}"),
            None => base_host.to_owned(),
        };

        if config.path_style || config.endpoint.is_some() {
            let canonical_uri = format!(
                "/{}/{}",
                uri_encode(&config.bucket, true),
                uri_encode(key, false)
            );
            return Ok(Self {
                url: format!("{scheme}://{base_host}{canonical_uri}"),
                host_header: base_host,
                canonical_uri,
            });
        }

        let host_header = format!("{}.{}", config.bucket, base_host);
        let canonical_uri = format!("/{}", uri_encode(key, false));

        Ok(Self {
            url: format!("{scheme}://{host_header}{canonical_uri}"),
            host_header,
            canonical_uri,
        })
    }
}

struct AwsTimestamp {
    short_date: String,
    amz_date: String,
}

impl AwsTimestamp {
    fn now() -> Self {
        let now = OffsetDateTime::now_utc();
        let month = month_number(now.month());
        let short_date = format!("{:04}{:02}{:02}", now.year(), month, now.day());
        let amz_date = format!(
            "{short_date}T{:02}{:02}{:02}Z",
            now.hour(),
            now.minute(),
            now.second()
        );

        Self {
            short_date,
            amz_date,
        }
    }
}

fn month_number(month: Month) -> u8 {
    match month {
        Month::January => 1,
        Month::February => 2,
        Month::March => 3,
        Month::April => 4,
        Month::May => 5,
        Month::June => 6,
        Month::July => 7,
        Month::August => 8,
        Month::September => 9,
        Month::October => 10,
        Month::November => 11,
        Month::December => 12,
    }
}

fn signing_key(secret_access_key: &str, short_date: &str, region: &str) -> Vec<u8> {
    let date_key = hmac_sha256(
        format!("AWS4{secret_access_key}").as_bytes(),
        short_date.as_bytes(),
    );
    let region_key = hmac_sha256(&date_key, region.as_bytes());
    let service_key = hmac_sha256(&region_key, b"s3");
    hmac_sha256(&service_key, b"aws4_request")
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::sign(&key, message).as_ref().to_vec()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn uri_encode(value: &str, encode_slash: bool) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            b'/' if !encode_slash => vec!['/'],
            other => format!("%{other:02X}").chars().collect(),
        })
        .collect()
}

fn response_header(response: &reqwest::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim_matches('"').to_owned())
        .filter(|value| !value.is_empty())
}

async fn ensure_success(status: StatusCode, action: &str, body: String) -> Result<()> {
    if status.is_success() {
        return Ok(());
    }

    let body = truncate_error(&body);
    bail!("{action} failed with {status}: {body}")
}

fn truncate_error(value: &str) -> String {
    const MAX_ERROR_BYTES: usize = 2_000;
    if value.len() <= MAX_ERROR_BYTES {
        return value.to_owned();
    }

    format!("{}...", &value[..MAX_ERROR_BYTES])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s3_object_key_uses_prefix_owner_database_and_backup() {
        let key = S3BackupObjectStoreConfig::new("bucket", "us-east-1")
            .with_prefix("liquid/backups/")
            .object_key("user-1", "db-1", "backup-1");

        assert_eq!(key, "liquid/backups/user-1/db-1/backup-1.dump");
    }

    #[test]
    fn uri_encoder_preserves_key_slashes() {
        assert_eq!(uri_encode("a b/c", false), "a%20b/c");
        assert_eq!(uri_encode("a/b", true), "a%2Fb");
    }
}
