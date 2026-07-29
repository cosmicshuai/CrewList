//! Server configuration, read from the environment. SPEC.md §6.1.

use std::env;
use std::net::SocketAddr;

/// Loopback by default. The trust model is *none* — anything that can open a
/// socket here has full control of the task list, which is safe precisely and
/// only because of this bind. SPEC.md §2.3, AC-62.
const DEFAULT_BIND: &str = "127.0.0.1:8787";

pub struct Config {
    pub bind: SocketAddr,
    pub postgres_url: String,
    pub mongo_url: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind = env::var("CREWLIST_BIND").unwrap_or_else(|_| DEFAULT_BIND.to_string());
        let bind: SocketAddr = bind
            .parse()
            .map_err(|e| anyhow::anyhow!("CREWLIST_BIND is not a socket address: {bind}: {e}"))?;

        Ok(Self {
            bind,
            postgres_url: required("CREWLIST_POSTGRES_URL")?,
            mongo_url: required("CREWLIST_MONGO_URL")?,
        })
    }
}

fn required(key: &str) -> anyhow::Result<String> {
    env::var(key).map_err(|_| anyhow::anyhow!("{key} must be set"))
}
