use reqwest::{Client, RequestBuilder};
use std::time::Duration;
use tracing::{debug, info};

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
        let domain = config.domain_with_trailing_slash();

        let mut builder = Client::builder()
            // Cloudflare answers 403 to lowercase header names even with valid cookies. HTTP/2 always
            // sends them lowercase, so this needs HTTP/1.1 with capitalized names.
            .http1_only()
            .http1_title_case_headers()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10));

        if let Some(proxy) = config.cdn_proxy.as_deref().filter(|p| !p.trim().is_empty()) {
            builder = builder.proxy(cdn_proxy(&domain, proxy)?);
            info!("Video CDN requests go through proxy {}", redact_proxy(proxy));
        }

        let client = builder.build()?;

        let user_agent = config
            .user_agent
            .clone()
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());

        Ok(Self {
            client,
            domain,
            user_agent,
            cookies: config.cookies.clone(),
        })
    }

    /// Whether `url` points at the configured site (e.g. chaturbate.com) rather than a CDN host.
    fn is_site_url(&self, url: &str) -> bool {
        is_same_site(&self.domain, url)
    }

    fn build_request(&self, url: &str) -> RequestBuilder {
        let mut req = self.client.get(url);

        // Cloudflare answers 403 when Host comes last, which is where hyper puts it by default.
        // Setting it here makes it the first header sent.
        if let Ok(parsed) = reqwest::Url::parse(url) {
            if let Some(host) = parsed.host_str() {
                let host_value = match parsed.port() {
                    Some(port) => format!("{}:{}", host, port),
                    None => host.to_string(),
                };
                req = req.header("Host", host_value);
            }
        }

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
        debug!("User-Agent: {}", self.user_agent);
        if let Some(ref cookies) = self.cookies {
            debug!("Cookies: {}...", &cookies.chars().take(50).collect::<String>());
        }
        let response = self.build_request(url).send().await?;

        let status = response.status();
        debug!("Response status: {} for {}", status, url);

        // Check for Cloudflare by looking for cf-ray header
        let is_cloudflare = response.headers().get("cf-ray").is_some();

        if status == reqwest::StatusCode::FORBIDDEN {
            // A 403 from anywhere other than the site itself (e.g. *.live.mmcdn.com) is the video
            // CDN refusing us, not an auth problem with chaturbate.com. Checked first so it never
            // feeds the monitor's cookie-death detection.
            if !self.is_site_url(url) {
                let host = reqwest::Url::parse(url)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_string))
                    .unwrap_or_else(|| url.to_string());
                debug!("CDN 403 from {}", host);
                return Err(Error::CdnForbidden(host));
            }
            if is_cloudflare {
                debug!("Cloudflare 403 detected (cf-ray header present)");
                return Err(Error::CloudflareBlocked);
            }
            return Err(Error::PrivateStream);
        }

        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(Error::RoomNotFound(url.to_string()));
        }

        if status.is_server_error() {
            return Err(Error::ServerError(
                status.as_u16(),
                format!("{} for {}", status.canonical_reason().unwrap_or("Unknown"), url),
            ));
        }

        let text = response.text().await?;

        // Check for Cloudflare challenge page
        if text.contains("<title>Just a moment...</title>") || text.contains("cf-challenge") {
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

/// Proxy that applies to every request except those to the site itself.
fn cdn_proxy(domain: &str, proxy: &str) -> Result<reqwest::Proxy> {
    let proxy_url = reqwest::Url::parse(proxy)
        .map_err(|e| Error::Config(format!("invalid cdn_proxy URL {:?}: {}", redact_proxy(proxy), e)))?;
    if !matches!(proxy_url.scheme(), "http" | "https") {
        return Err(Error::Config(format!(
            "cdn_proxy must be an http:// or https:// URL, got {}://",
            proxy_url.scheme()
        )));
    }
    let site = domain.to_string();
    Ok(reqwest::Proxy::custom(move |url| {
        if is_same_site(&site, url.as_str()) {
            None
        } else {
            Some(proxy_url.clone())
        }
    }))
}

/// Proxy URL with any user:password replaced, for logs and errors.
fn redact_proxy(proxy: &str) -> String {
    match reqwest::Url::parse(proxy) {
        Ok(mut u) if !u.username().is_empty() || u.password().is_some() => {
            let _ = u.set_username("***");
            let _ = u.set_password(None);
            u.to_string()
        }
        Ok(u) => u.to_string(),
        Err(_) => "<unparseable>".to_string(),
    }
}

/// True when `url`'s host is the host of `domain` or one of its subdomains.
fn is_same_site(domain: &str, url: &str) -> bool {
    let host_of = |u: &str| {
        reqwest::Url::parse(u)
            .ok()
            .and_then(|p| p.host_str().map(|h| h.trim_start_matches("www.").to_ascii_lowercase()))
    };
    match (host_of(domain), host_of(url)) {
        (Some(site), Some(host)) => host == site || host.ends_with(&format!(".{}", site)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{cdn_proxy, is_same_site, redact_proxy};

    #[test]
    fn site_vs_cdn_hosts() {
        let d = "https://chaturbate.com/";
        assert!(is_same_site(d, "https://chaturbate.com/someroom/"));
        assert!(is_same_site(d, "https://www.chaturbate.com/api/x/"));
        assert!(!is_same_site(
            d,
            "https://edge19-waw.live.mmcdn.com/v1/edge/streams/origin.x/llhls.m3u8?token=t"
        ));
        assert!(!is_same_site(d, "https://notchaturbate.com/"));
        assert!(!is_same_site(d, "not a url"));
    }

    #[test]
    fn cdn_proxy_validation() {
        let d = "https://chaturbate.com/";
        assert!(cdn_proxy(d, "http://proxy.example:3128").is_ok());
        assert!(cdn_proxy(d, "not a url").is_err());
        assert!(cdn_proxy(d, "ftp://proxy.example:21").is_err());
    }

    #[test]
    fn proxy_credentials_are_redacted() {
        let r = redact_proxy("http://user:secret@proxy.example:3128");
        assert!(!r.contains("secret") && !r.contains("user"), "{}", r);
    }
}
