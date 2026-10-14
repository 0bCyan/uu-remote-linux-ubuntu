//! UU Remote Official Authentication & REST Client

use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

/// Request-signing key built into the official UU PC client (x-param-plat=1); the same for
/// every user. Config "sign_key" / UU_SIGN_KEY override it if NetEase rotates the key.
pub const DEFAULT_SIGN_KEY: &str = "alWiSzXZTLu3WfFnw13uBru3";
/// API code for an invalid or expired login token ("Token无效或已过期", HTTP 422).
pub const TOKEN_INVALID: i64 = 1120;

#[derive(Debug)]
pub enum RoomError { LoginExpired, Other(String) }
impl std::fmt::Display for RoomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { RoomError::LoginExpired => f.write_str("login token invalid or expired"), RoomError::Other(e) => f.write_str(e) }
    }
}

pub const BASE_URL: &str = "https://api.nrd.nie.163.com";

/// Account binding and request-signing key. Never committed: loaded at startup from
/// a private config file or environment variables (see config.example.json).
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Credentials {
    pub user_id: String,
    pub device_id: String,
    pub client_id: String,
    pub system_id: String,
    pub token: String,
    pub csrf: String,
    /// HMAC-SHA256 key for x-param-sign.
    pub sign_key: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credentials { .. }")
    }
}

impl Credentials {
    /// Environment variables win over the file (UU_USER_ID, UU_DEVICE_ID, UU_CLIENT_ID,
    /// UU_SYSTEM_ID, UU_TOKEN, UU_CSRF, UU_SIGN_KEY).
    pub fn load(file: Option<&serde_json::Value>) -> Result<Self, String> {
        let get = |key: &str| -> String {
            std::env::var(format!("UU_{}", key.to_uppercase())).ok().filter(|v| !v.is_empty())
                .or_else(|| file.and_then(|f| f.get(key)).and_then(|v| v.as_str()).map(str::to_owned))
                .unwrap_or_default()
        };
        let creds = Self {
            user_id: get("user_id"), device_id: get("device_id"), client_id: get("client_id"),
            system_id: get("system_id"), token: get("token"), csrf: get("csrf"),
            sign_key: Some(get("sign_key")).filter(|k| !k.is_empty()).unwrap_or_else(|| DEFAULT_SIGN_KEY.into()),
        };
        let missing: Vec<&str> = [("user_id", &creds.user_id), ("device_id", &creds.device_id), ("client_id", &creds.client_id),
            ("system_id", &creds.system_id), ("token", &creds.token), ("csrf", &creds.csrf), ("sign_key", &creds.sign_key)]
            .iter().filter(|(_, v)| v.is_empty()).map(|(k, _)| *k).collect();
        if missing.is_empty() { Ok(creds) } else { Err(format!("missing credential settings: {}", missing.join(", "))) }
    }
}

#[derive(Debug, Deserialize)]
pub struct ApiResponse<T> {
    pub code: i32,
    pub msg: String,
    pub data: Option<T>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoomData {
    pub signaling_server: String,
    pub token: String,
    pub signaling_list: Option<Vec<String>>,
    pub ws_connect_timeout_ms: Option<u64>,
}

pub struct UUAuthClient {
    pub creds: Credentials,
    http: reqwest::Client,
}

impl UUAuthClient {
    pub fn new(creds: Credentials) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(12))
            .build()
            .unwrap();
        Self { creds, http }
    }

    /// 计算网易 NRD HMAC-SHA256 签名
    pub fn sign_request(
        &self,
        method: &str,
        path: &str,
        headers: &BTreeMap<String, String>,
        body_str: &str,
    ) -> String {
        let mut filtered: BTreeMap<String, String> = BTreeMap::new();
        for (k, v) in headers {
            let kl = k.to_lowercase();
            if kl.starts_with("x-param-") && kl != "x-param-sign" {
                filtered.insert(kl, v.clone());
            }
        }
        let params_str = filtered
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        let string_to_sign = format!("{}{}{}{}", method.to_uppercase(), path, params_str, body_str);
        let mut mac = HmacSha256::new_from_slice(self.creds.sign_key.as_bytes()).expect("HMAC can take key");
        mac.update(string_to_sign.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    pub fn build_headers(
        &self,
        method: &str,
        path: &str,
        body_str: &str,
    ) -> HeaderMap {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let ts = now.to_string();

        let mut param_map = BTreeMap::new();
        param_map.insert("x-param-plat".into(), "1".into());
        param_map.insert("x-param-vn".into(), "4.42.0.2770".into());
        param_map.insert("x-param-vc".into(), "2770".into());
        param_map.insert("x-param-pkgn".into(), "com.netease.gameviewer".into());
        param_map.insert("x-param-chn".into(), "gwqd".into());
        param_map.insert("x-param-lang".into(), "zh-CN".into());
        param_map.insert("x-param-cnt".into(), "CN".into());
        // Before login some identifiers do not exist yet; absent ones are not sent or signed.
        for (key, value) in [("x-param-client-id", &self.creds.client_id), ("x-param-device-id", &self.creds.device_id),
            ("x-param-system-id", &self.creds.system_id), ("x-param-user-id", &self.creds.user_id)] {
            if !value.is_empty() { param_map.insert(key.into(), value.clone()); }
        }
        param_map.insert("x-param-ts".into(), ts);

        let sign = self.sign_request(method, path, &param_map, body_str);

        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("application/json"));
        headers.insert("Accept", HeaderValue::from_static("application/json"));
        headers.insert(
            "User-Agent",
            HeaderValue::from_static("UURemote/4.42.0 (Windows NT 10.0; Win64; x64)"),
        );
        if !self.creds.token.is_empty() {
            if let Ok(v) = HeaderValue::from_str(&format!("Bearer {}", self.creds.token)) { headers.insert("Authorization", v); }
            if let Ok(v) = HeaderValue::from_str(&format!("nrd_access_token={}; nrd_csrf_access_token={}", self.creds.token, self.creds.csrf)) {
                headers.insert("Cookie", v);
            }
            if let Ok(v) = HeaderValue::from_str(&self.creds.csrf) { headers.insert("X-CSRF-TOKEN", v); }
        }

        for (k, v) in param_map {
            if let Ok(name) = HeaderName::from_bytes(k.as_bytes()) {
                if let Ok(val) = HeaderValue::from_str(&v) {
                    headers.insert(name, val);
                }
            }
        }
        headers.insert("X-Param-SIGN", HeaderValue::from_str(&sign).unwrap());

        headers
    }

    /// Signed JSON POST; returns (HTTP status, decoded body).
    pub async fn post_json(&self, path: &str, body: &serde_json::Value) -> Result<(u16, serde_json::Value), String> {
        let body_str = serde_json::to_string(body).map_err(|e| e.to_string())?;
        let resp = self.http.post(format!("{}{}", BASE_URL, path))
            .headers(self.build_headers("POST", path, &body_str)).body(body_str)
            .send().await.map_err(|e| format!("HTTP post {path} failed: {e}"))?;
        let status = resp.status().as_u16();
        let value = resp.json().await.unwrap_or(serde_json::Value::Null);
        Ok((status, value))
    }

    /// 向网易服务器请求创建房间，获取最新的信令服务器与鉴权 Token
    pub async fn create_room(&self) -> Result<RoomData, RoomError> {
        let (status, body) = self.post_json("/api/v1/room/create", &serde_json::json!({})).await.map_err(RoomError::Other)?;
        let code = body.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code == TOKEN_INVALID { return Err(RoomError::LoginExpired); }
        if status == 200 && code == 0 {
            if let Some(data) = body.get("data") {
                let room: RoomData = serde_json::from_value(data.clone()).map_err(|e| RoomError::Other(format!("room data decode failed: {e}")))?;
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                log::info!("Room token lifetime: remaining_s={:?}", room_token_lifetime(&room.token, now));
                return Ok(room);
            }
        }
        Err(RoomError::Other(format!("create_room error http {status} code {code}: {}",
            body.get("msg").and_then(|m| m.as_str()).unwrap_or(""))))
    }
}

// Report only the remaining lifetime; never return or log token/claim contents.
fn room_token_lifetime(token: &str, now: u64) -> Option<i64> {
    use base64::Engine as _;
    let payload = token.split('.').nth(1)?;
    if payload.len() > 16384 { return None; }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload)).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let remaining = claims.get("exp")?.as_i64()?.saturating_sub(i64::try_from(now).ok()?);
    (remaining.unsigned_abs() <= 7 * 24 * 3600).then_some(remaining)
}

#[cfg(test)]
mod room_lifetime_tests {
    use super::room_token_lifetime;
    use base64::Engine as _;
    #[test]
    fn exposes_only_remaining_seconds_and_handles_opaque_tokens() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"exp":2200,"ignored":"test-only"}"#);
        assert_eq!(room_token_lifetime(&format!("h.{payload}.s"),1000),Some(1200));
        assert_eq!(room_token_lifetime("opaque",1000),None);
        assert_eq!(room_token_lifetime("h.invalid.s",1000),None);
    }
}
