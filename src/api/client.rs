use reqwest::{Client, RequestBuilder};
use std::time::Duration;
use tracing::debug;

use crate::config::NetworkConfig;
use crate::error::{Error, Result};

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub struct ChaturbateClient {
    client: Client,
    domain: String,
    user_agent: String,
    cookies: Option<String>,
}

impl ChaturbateClient {
    pub fn new(config: &NetworkConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()?;

        let user_agent = config
            .user_agent
            .clone()
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());

        Ok(Self {
            client,
            domain: config.domain_with_trailing_slash(),
            user_agent,
            cookies: config.cookies.clone(),
        })
    }

    fn build_request(&self, url: &str) -> RequestBuilder {
        let mut req = self.client.get(url);

        // Browser-like headers to avoid Cloudflare blocks
        req = req.header("User-Agent", &self.user_agent);
        req = req.header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8");
        req = req.header("Accept-Language", "en-US,en;q=0.9");
        req = req.header("Accept-Encoding", "gzip, deflate, br");
        req = req.header("Sec-Ch-Ua", "\"Chromium\";v=\"120\", \"Not(A:Brand\";v=\"24\"");
        req = req.header("Sec-Ch-Ua-Mobile", "?0");
        req = req.header("Sec-Ch-Ua-Platform", "\"Windows\"");
        req = req.header("Sec-Fetch-Dest", "document");
        req = req.header("Sec-Fetch-Mode", "navigate");
        req = req.header("Sec-Fetch-Site", "none");
        req = req.header("Sec-Fetch-User", "?1");
        req = req.header("Upgrade-Insecure-Requests", "1");
        // Required header to bypass age verification
        req = req.header("X-Requested-With", "XMLHttpRequest");

        if let Some(ref cookies) = self.cookies {
            req = req.header("Cookie", cookies);
        }

        req
    }

    pub async fn get(&self, url: &str) -> Result<String> {
        debug!("GET {}", url);
        let response = self.build_request(url).send().await?;

        let status = response.status();
        debug!("Response status: {} for {}", status, url);

        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(Error::PrivateStream);
        }

        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(Error::RoomNotFound(url.to_string()));
        }

        let text = response.text().await?;

        // Check for Cloudflare protection
        if text.contains("<title>Just a moment...</title>") {
            return Err(Error::CloudflareBlocked);
        }

        // Check for age verification
        if text.contains("Verify your age") {
            return Err(Error::AgeVerification);
        }

        Ok(text)
    }

    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let response = self.build_request(url).send().await?;

        let status = response.status();
        if !status.is_success() {
            return Err(Error::Network(
                response.error_for_status().unwrap_err()
            ));
        }

        Ok(response.bytes().await?.to_vec())
    }

    pub async fn get_room_page(&self, room: &str) -> Result<String> {
        let url = format!("{}{}/", self.domain, room);
        debug!("Fetching room page: {}", url);
        self.get(&url).await
    }

    pub fn domain(&self) -> &str {
        &self.domain
    }
}

impl Clone for ChaturbateClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            domain: self.domain.clone(),
            user_agent: self.user_agent.clone(),
            cookies: self.cookies.clone(),
        }
    }
}
