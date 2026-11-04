use reqwest::cookie::{CookieStore, Jar};
use reqwest::{Client, RequestBuilder};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

use crate::config::NetworkConfig;
use crate::error::{Error, Result};

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub struct ChaturbateClient {
    client: Client,
    domain: String,
    user_agent: String,
    /// Cookie jar shared by all clones. Seeded from the configured cookie string and
    /// updated from Set-Cookie, like a browser: Cloudflare refreshes __cf_bm (and
    /// sometimes cf_clearance) on every response, and resending a stale copy raises the
    /// bot score. The jar also scopes cookies to their domain, so the site's session
    /// cookies are no longer sent to the video CDN.
    jar: Arc<Jar>,
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

        let jar = Arc::new(Jar::default());
        if let Some(cookies) = config.cookies.as_deref() {
            let seeded = seed_cookie_jar(&jar, &domain, cookies)?;
            debug!("Seeded {} cookie(s) for {}", seeded, domain);
        }
        builder = builder.cookie_provider(Arc::clone(&jar));

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
            jar,
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

        // Cookies come from the jar (cookie_provider), scoped to each request's domain.
        req
    }

    pub async fn get(&self, url: &str) -> Result<String> {
        debug!("GET {}", url);
        debug!("User-Agent: {}", self.user_agent);
        if let Some(names) = self.cookie_names_for(url) {
            debug!("Cookies sent: {}", names);
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
                debug!(
                    "Cloudflare 403 for {} (cf-mitigated: {})",
                    url,
                    response
                        .headers()
                        .get("cf-mitigated")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("none")
                );
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

    /// Names (not values) of the cookies the jar would send to `url`, for debug logs.
    fn cookie_names_for(&self, url: &str) -> Option<String> {
        let url = reqwest::Url::parse(url).ok()?;
        let header = self.jar.cookies(&url)?;
        let names: Vec<&str> = header
            .to_str()
            .ok()?
            .split(';')
            .filter_map(|c| c.trim().split('=').next())
            .filter(|n| !n.is_empty())
            .collect();
        Some(names.join(","))
    }
}

impl Clone for ChaturbateClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            domain: self.domain.clone(),
            user_agent: self.user_agent.clone(),
            jar: Arc::clone(&self.jar),
        }
    }
}

/// Add a "k1=v1; k2=v2" cookie string to the jar for the site. Each cookie gets
/// `Domain=<site host>; Path=/`, matching what Cloudflare and the site set, so a later
/// Set-Cookie for the same name replaces it instead of adding a second copy. Hosts that
/// are IP addresses (tests, local setups) get host-only cookies. Returns how many were added.
fn seed_cookie_jar(jar: &Jar, domain: &str, cookies: &str) -> Result<usize> {
    let url = reqwest::Url::parse(domain)
        .map_err(|e| Error::Config(format!("invalid domain {:?}: {}", domain, e)))?;
    let host = url
        .host_str()
        .ok_or_else(|| Error::Config(format!("domain has no host: {}", domain)))?;
    let domain_attr = if host.parse::<std::net::IpAddr>().is_ok() {
        String::new()
    } else {
        format!("; Domain={}", host.trim_start_matches("www."))
    };

    let mut added = 0;
    for pair in cookies.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        if !pair.contains('=') {
            return Err(Error::Config(format!(
                "malformed cookie {:?}: expected name=value pairs separated by ';'",
                pair.chars().take(20).collect::<String>()
            )));
        }
        jar.add_cookie_str(&format!("{}{}; Path=/", pair, domain_attr), &url);
        added += 1;
    }
    Ok(added)
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
    use super::{cdn_proxy, is_same_site, redact_proxy, seed_cookie_jar, ChaturbateClient};
    use crate::config::NetworkConfig;
    use reqwest::cookie::{CookieStore, Jar};
    use reqwest::header::HeaderValue;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn cookie_header(jar: &Jar, url: &str) -> String {
        jar.cookies(&reqwest::Url::parse(url).unwrap())
            .map(|h| h.to_str().unwrap().to_string())
            .unwrap_or_default()
    }

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

    #[test]
    fn seeded_cookies_go_to_site_only() {
        let jar = Jar::default();
        let n = seed_cookie_jar(&jar, "https://chaturbate.com/", "sessionid=s1; cf_clearance=c1; affkey=\"a=b\"").unwrap();
        assert_eq!(n, 3);
        let site = cookie_header(&jar, "https://chaturbate.com/room/");
        assert!(site.contains("sessionid=s1") && site.contains("cf_clearance=c1"), "{}", site);
        assert!(cookie_header(&jar, "https://www.chaturbate.com/").contains("sessionid=s1"));
        assert_eq!(cookie_header(&jar, "https://edge1-waw.live.mmcdn.com/x.m3u8"), "");
    }

    #[test]
    fn set_cookie_replaces_seeded_value() {
        let jar = Jar::default();
        seed_cookie_jar(&jar, "https://chaturbate.com/", "__cf_bm=old; sessionid=s1").unwrap();
        let url = reqwest::Url::parse("https://chaturbate.com/room/").unwrap();
        let set = HeaderValue::from_static("__cf_bm=new; Domain=.chaturbate.com; Path=/; HttpOnly; Secure");
        jar.set_cookies(&mut std::iter::once(&set), &url);
        let h = cookie_header(&jar, "https://chaturbate.com/room/");
        assert!(h.contains("__cf_bm=new") && !h.contains("__cf_bm=old"), "{}", h);
        assert_eq!(h.matches("__cf_bm=").count(), 1, "{}", h);
        assert!(h.contains("sessionid=s1"), "{}", h);
    }

    #[test]
    fn malformed_cookie_string_is_rejected() {
        let jar = Jar::default();
        assert!(seed_cookie_jar(&jar, "https://chaturbate.com/", "sessionid=s1; garbage").is_err());
    }

    /// End to end against a local server: the first response sets a new __cf_bm, the
    /// second request must carry it (and still carry the seeded session cookie).
    #[tokio::test]
    async fn client_sends_back_cookies_the_server_set() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut seen = Vec::new();
            for i in 0..2 {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap();
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let cookie = req
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("cookie:"))
                    .unwrap_or("")
                    .to_string();
                seen.push(cookie);
                let set = if i == 0 { "Set-Cookie: __cf_bm=fresh; Path=/\r\n" } else { "" };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\n{}Content-Length: 2\r\nConnection: close\r\n\r\nok",
                    set
                );
                sock.write_all(resp.as_bytes()).await.unwrap();
            }
            seen
        });

        let config = NetworkConfig {
            cookies: Some("__cf_bm=stale; sessionid=s1".to_string()),
            domain: format!("http://{}/", addr),
            ..NetworkConfig::default()
        };
        let client = ChaturbateClient::new(&config).unwrap();
        let url = format!("http://{}/room/", addr);
        client.get(&url).await.unwrap();
        client.clone().get(&url).await.unwrap(); // clones share the jar

        let seen = server.await.unwrap();
        assert!(seen[0].contains("__cf_bm=stale") && seen[0].contains("sessionid=s1"), "{}", seen[0]);
        assert!(seen[1].contains("__cf_bm=fresh") && !seen[1].contains("stale"), "{}", seen[1]);
        assert!(seen[1].contains("sessionid=s1"), "{}", seen[1]);
    }
}
