//! Minimal InnerTube (YouTube Music web client) access.

use crate::auth::{Cookies, ORIGIN};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::time::Duration;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";

pub struct Client {
    http: reqwest::Client,
    /// `None` = anonymous (public data only; used for test fixtures).
    cookies: Option<Cookies>,
    client_version: String,
}

impl std::fmt::Debug for Client {
    // No cookies in debug output.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("client_version", &self.client_version)
            .field("signed_in", &self.cookies.is_some())
            .finish_non_exhaustive()
    }
}

impl Client {
    /// Signed-in client using the given browser cookies.
    pub async fn new(cookies: Cookies) -> Result<Self> {
        Self::connect(Some(cookies)).await
    }

    /// Signed-out client: public pages only (search, artists, albums, public playlists).
    pub async fn anonymous() -> Result<Self> {
        Self::connect(None).await
    }

    async fn connect(cookies: Option<Cookies>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        let mut req = http.get(ORIGIN);
        if let Some(c) = &cookies {
            req = req.header("Cookie", c.header());
        }
        let page = req
            .send()
            .await
            .context("can't reach music.youtube.com (offline?)")?
            .error_for_status()?
            .text()
            .await?;
        let client_version = extract(&page, "\"INNERTUBE_CLIENT_VERSION\":\"")
            .context("client version not found on music.youtube.com")?;
        Ok(Self { http, cookies, client_version })
    }

    pub fn client_version(&self) -> &str {
        &self.client_version
    }

    pub fn is_signed_in(&self) -> bool {
        self.cookies.is_some()
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub(crate) fn cookie_header(&self) -> &str {
        self.cookies.as_ref().map(|c| c.header()).unwrap_or("")
    }

    /// POST /youtubei/v1/{endpoint} (may include a query string) with the web-remix context
    /// merged into `body`. Transient failures (network, 5xx, 429) are retried once.
    pub async fn call(&self, endpoint: &str, body: Value) -> Result<Value> {
        match self.call_once(endpoint, body.clone()).await {
            Err(e) if is_transient(&e) => {
                tokio::time::sleep(Duration::from_millis(700)).await;
                self.call_once(endpoint, body).await
            }
            r => r,
        }
    }

    async fn call_once(&self, endpoint: &str, mut body: Value) -> Result<Value> {
        body["context"] = json!({
            "client": { "clientName": "WEB_REMIX", "clientVersion": self.client_version, "hl": "en", "gl": "TR" },
            "user": {}
        });
        let url = format!("{ORIGIN}/youtubei/v1/{endpoint}{}prettyPrint=false", if endpoint.contains('?') { '&' } else { '?' });
        let mut req = self.http.post(url).header("Origin", ORIGIN).header("X-Origin", ORIGIN).json(&body);
        if let Some(c) = &self.cookies {
            req = req
                .header("Cookie", c.header())
                .header("Authorization", c.sapisid_hash())
                .header("X-Goog-AuthUser", "0");
        }
        let resp = req.send().await.map_err(|e| anyhow::Error::new(Transient(e.to_string())))?;
        let status = resp.status();
        let value: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let msg = value["error"]["message"].as_str().unwrap_or("no details").to_string();
            let name = endpoint.split('?').next().unwrap_or(endpoint);
            let err = anyhow::anyhow!("YouTube Music returned HTTP {status} for {name}: {msg}");
            if status.is_server_error() || status.as_u16() == 429 {
                return Err(err.context(Transient("server".into())));
            }
            return Err(err);
        }
        Ok(value)
    }

    pub async fn browse(&self, browse_id: &str) -> Result<Value> {
        self.call("browse", json!({ "browseId": browse_id })).await
    }
}

/// Marker for errors worth one retry.
#[derive(Debug)]
struct Transient(String);

impl std::fmt::Display for Transient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "network error: {}", self.0)
    }
}

impl std::error::Error for Transient {}

fn is_transient(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.is::<Transient>())
}

fn extract(haystack: &str, prefix: &str) -> Option<String> {
    let start = haystack.find(prefix)? + prefix.len();
    let len = haystack[start..].find('"')?;
    Some(haystack[start..start + len].to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn extracts_version() {
        let page = r#"..."INNERTUBE_CLIENT_VERSION":"1.20261117.06.00","x":1"#;
        assert_eq!(super::extract(page, "\"INNERTUBE_CLIENT_VERSION\":\""), Some("1.20261117.06.00".into()));
    }

    #[test]
    fn transient_detection() {
        let e = anyhow::Error::new(super::Transient("timeout".into()));
        assert!(super::is_transient(&e));
        assert!(!super::is_transient(&anyhow::anyhow!("bad request")));
    }
}
