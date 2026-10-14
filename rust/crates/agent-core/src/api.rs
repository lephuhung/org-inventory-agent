//! Port of ApiClient.cs — JSON over HTTPS with optional mTLS client cert.
//! Never forges `X-SSL-*` headers; identity comes from the TLS client certificate only.

use crate::endpoint::EndpointManager;
use crate::identity;
use crate::keystore::KeyStore;
use crate::SharedConfig;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;

const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone)]
pub struct ApiResponse {
    pub status: u16,
    pub ok: bool,
    pub body: Option<Value>,
    pub detail: Option<String>,
}

impl ApiResponse {
    pub fn str_field(&self, key: &str) -> Option<String> {
        self.body.as_ref()?.get(key)?.as_str().map(str::to_string)
    }
    /// Positive integer field, like `TryGetInt` in the .NET services.
    pub fn pos_int(&self, key: &str) -> Option<i32> {
        let v = self.body.as_ref()?.get(key)?.as_i64()?;
        (v > 0 && v <= i32::MAX as i64).then_some(v as i32)
    }
}

#[derive(Debug)]
pub enum ApiError {
    /// Network/TLS/timeout failure after retries (ApiTransportException).
    Transport(String),
    /// Client could not be built (not enrolled, cert missing, bad proxy …).
    Setup(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Transport(m) | ApiError::Setup(m) => f.write_str(m),
        }
    }
}
impl std::error::Error for ApiError {}

pub struct ApiClient {
    config: SharedConfig,
    endpoints: Arc<EndpointManager>,
    keystore: KeyStore,
    user_agent: String,
    cached: AsyncMutex<Option<(Option<String>, reqwest::Client)>>,
}

impl ApiClient {
    pub fn new(config: SharedConfig, endpoints: Arc<EndpointManager>, keystore: KeyStore) -> Self {
        Self {
            config,
            endpoints,
            keystore,
            user_agent: format!("OrgInventoryAgent/{}", crate::VERSION),
            cached: AsyncMutex::new(None),
        }
    }

    pub fn endpoints(&self) -> &Arc<EndpointManager> {
        &self.endpoints
    }

    /// Drop the cached client so the next request re-reads the cert (after renew/enroll).
    pub async fn reset(&self) {
        *self.cached.lock().await = None;
    }

    pub async fn post_json<T: Serialize>(
        &self,
        path: &str,
        payload: &T,
        mtls: bool,
        timeout_s: u64,
    ) -> Result<ApiResponse, ApiError> {
        let json = serde_json::to_string(payload).map_err(|e| ApiError::Setup(e.to_string()))?;
        self.send(reqwest::Method::POST, path, Some(json), mtls, timeout_s)
            .await
    }

    pub async fn get_json(
        &self,
        path: &str,
        mtls: bool,
        timeout_s: u64,
    ) -> Result<ApiResponse, ApiError> {
        self.send(reqwest::Method::GET, path, None, mtls, timeout_s)
            .await
    }

    /// Single attempt POST of a pre-serialized body to an absolute URL (offline cache replay).
    pub async fn post_raw_json(
        &self,
        url: &str,
        body: &str,
        mtls: bool,
        timeout_s: u64,
    ) -> Result<ApiResponse, ApiError> {
        let client = self.client(mtls).await?;
        log::info!(target: "ApiClient", "[KẾT NỐI] Gửi bù offline POST {url} (mTLS={mtls}, Payload={}B)...", body.len());
        let started = Instant::now();
        match self
            .request(
                &client,
                reqwest::Method::POST,
                url,
                Some(body.to_string()),
                mtls,
                timeout_s,
            )
            .await
        {
            Ok(resp) => {
                self.endpoints.on_success();
                log::info!(target: "ApiClient", "[KẾT NỐI] Gửi bù offline hoàn tất {url} -> HTTP {} ({}ms)", resp.status, started.elapsed().as_millis());
                Ok(resp)
            }
            Err(e) => {
                self.endpoints.on_failure();
                log::warn!(target: "ApiClient", "[KẾT NỐI] Gửi bù offline tới {url} thất bại sau {}ms: {e}", started.elapsed().as_millis());
                Err(ApiError::Transport(format!(
                    "Không kết nối được {url}: {e}"
                )))
            }
        }
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        json: Option<String>,
        mtls: bool,
        timeout_s: u64,
    ) -> Result<ApiResponse, ApiError> {
        let url = self
            .endpoints
            .build_url(path)
            .map_err(|e| ApiError::Setup(e.to_string()))?;
        let client = self.client(mtls).await?;
        log::info!(
            target: "ApiClient",
            "[KẾT NỐI] Gửi HTTP {method} {url} (mTLS={mtls}, Payload={}B)...",
            json.as_ref().map_or(0, String::len)
        );
        let mut attempt = 0;
        loop {
            let started = Instant::now();
            match self
                .request(&client, method.clone(), &url, json.clone(), mtls, timeout_s)
                .await
            {
                Ok(resp) => {
                    let ms = started.elapsed().as_millis();
                    if resp.status >= 500 || resp.status == 429 {
                        log::warn!(
                            target: "ApiClient",
                            "[KẾT NỐI] Server trả về HTTP {} sau {ms}ms (lần thử {}/{MAX_ATTEMPTS}): {}",
                            resp.status, attempt + 1, resp.detail.as_deref().unwrap_or("N/A")
                        );
                        self.endpoints.on_failure();
                        if attempt + 1 >= MAX_ATTEMPTS {
                            return Ok(resp);
                        }
                    } else {
                        self.endpoints.on_success();
                        if resp.ok {
                            log::info!(target: "ApiClient", "[KẾT NỐI] Thành công {method} {url} -> HTTP {} ({ms}ms)", resp.status);
                        } else {
                            log::warn!(
                                target: "ApiClient",
                                "[KẾT NỐI] Phản hồi HTTP {} từ {url} sau {ms}ms: {}",
                                resp.status, resp.detail.as_deref().unwrap_or("N/A")
                            );
                        }
                        return Ok(resp);
                    }
                }
                Err(e) => {
                    self.endpoints.on_failure();
                    log::warn!(
                        target: "ApiClient",
                        "[KẾT NỐI] Lỗi kết nối tới {url} (lần thử {}/{MAX_ATTEMPTS}, sau {}ms): {e}",
                        attempt + 1, started.elapsed().as_millis()
                    );
                    if attempt + 1 >= MAX_ATTEMPTS {
                        log::error!(target: "ApiClient", "[KẾT NỐI] Thất bại toàn bộ {MAX_ATTEMPTS} lần thử tới {url}: {e}");
                        return Err(ApiError::Transport(format!(
                            "Không kết nối được {url}: {e}"
                        )));
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
            attempt += 1;
        }
    }

    async fn request(
        &self,
        client: &reqwest::Client,
        method: reqwest::Method,
        url: &str,
        json: Option<String>,
        mtls: bool,
        timeout_s: u64,
    ) -> Result<ApiResponse, String> {
        let mut req = client
            .request(method, url)
            .timeout(Duration::from_secs(timeout_s));
        if mtls {
            if let Some(mid) = self
                .config
                .lock()
                .unwrap()
                .machine_id
                .clone()
                .filter(|m| !m.trim().is_empty())
            {
                req = req.header("X-Machine-Id", mid);
            }
        }
        if let Some(body) = json {
            req = req
                .header(
                    reqwest::header::CONTENT_TYPE,
                    "application/json; charset=utf-8",
                )
                .body(body);
        }
        let resp = req.send().await.map_err(describe)?;
        let status = resp.status();
        let text = resp.text().await.map_err(describe)?;
        Ok(to_api_response(status.as_u16(), status.is_success(), &text))
    }

    async fn client(&self, mtls: bool) -> Result<reqwest::Client, ApiError> {
        let mut cached = self.cached.lock().await;
        let (thumb, identity, proxy) = {
            let mut cfg = self.config.lock().unwrap();
            let mut identity = None;
            let mut thumb = None;
            if mtls {
                if !identity::is_enrolled(&cfg) {
                    return Err(ApiError::Setup(
                        "Chưa enroll — không có client cert cho request mTLS.".into(),
                    ));
                }
                thumb = cfg.client_cert_thumbprint.clone();
                if cached.as_ref().is_some_and(|(t, _)| *t == thumb) {
                    return Ok(cached.as_ref().unwrap().1.clone());
                }
                identity = self.keystore.find(&mut cfg);
            } else if cached.as_ref().is_some_and(|(t, _)| t.is_none()) {
                return Ok(cached.as_ref().unwrap().1.clone());
            }
            (thumb, identity, cfg.http_proxy.clone())
        };
        let mut b = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .gzip(true)
            .deflate(true)
            .user_agent(&self.user_agent)
            .default_headers({
                let mut h = reqwest::header::HeaderMap::new();
                h.insert(
                    reqwest::header::ACCEPT,
                    reqwest::header::HeaderValue::from_static("application/json"),
                );
                h
            });
        if let Some(p) = proxy.filter(|p| !p.trim().is_empty()) {
            b = b.proxy(
                reqwest::Proxy::all(p.trim())
                    .map_err(|e| ApiError::Setup(format!("HttpProxy không hợp lệ: {e}")))?,
            );
        }
        if mtls {
            match identity {
                Some(c) => {
                    let id = reqwest::Identity::from_pem(
                        format!("{}\n{}", c.cert_pem.trim(), c.key_pem.trim()).as_bytes(),
                    )
                    .map_err(|e| ApiError::Setup(format!("Client cert không dùng được: {e}")))?;
                    log::debug!(target: "ApiClient", "mTLS client cert: {}", c.subject);
                    b = b.identity(id);
                }
                None => {
                    if self
                        .endpoints
                        .current()
                        .is_some_and(|c| c.to_lowercase().starts_with("https://"))
                    {
                        return Err(ApiError::Setup(
                            "Không tìm thấy client cert trong store cho kênh HTTPS mTLS.".into(),
                        ));
                    }
                }
            }
        }
        let client = b.build().map_err(|e| ApiError::Setup(e.to_string()))?;
        *cached = Some((if mtls { thumb } else { None }, client.clone()));
        Ok(client)
    }
}

fn describe(e: reqwest::Error) -> String {
    use std::error::Error;
    let mut msg = e.to_string();
    let mut src = e.source();
    while let Some(s) = src {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        src = s.source();
    }
    msg
}

pub fn to_api_response(status: u16, ok: bool, text: &str) -> ApiResponse {
    let body: Option<Value> = if text.trim().is_empty() {
        None
    } else {
        serde_json::from_str(text).ok()
    };
    let detail = body
        .as_ref()
        .and_then(|b| b.get("detail"))
        .map(|d| match d {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        });
    ApiResponse {
        status,
        ok,
        body,
        detail,
    }
}
