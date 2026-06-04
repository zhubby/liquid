use std::{env, net::SocketAddr};

use anyhow::{Context, Result};

const DEFAULT_API_ADDR: &str = "127.0.0.1:3001";
const DEFAULT_DATABASE_URL: &str = "postgres://postgres:postgres@localhost:5432/liquid";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidConfig {
    pub api_addr: SocketAddr,
    pub database_url: String,
}

impl LiquidConfig {
    pub fn from_env() -> Result<Self> {
        let api_addr = env::var("LIQUID_API_ADDR").unwrap_or_else(|_| DEFAULT_API_ADDR.to_owned());
        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned());

        Ok(Self {
            api_addr: api_addr
                .parse()
                .with_context(|| format!("invalid LIQUID_API_ADDR: {api_addr}"))?,
            database_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let addr: SocketAddr = DEFAULT_API_ADDR.parse().expect("default api addr");

        assert_eq!(addr.port(), 3001);
        assert!(DEFAULT_DATABASE_URL.starts_with("postgres://"));
    }
}
