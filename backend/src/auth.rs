use crate::{
    error::{ApiError, Result},
    AppState,
};
use axum::{
    extract::FromRequestParts,
    http::{request::Parts, HeaderMap},
};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Auth {
    pub app_id: Option<String>,
    client: reqwest::Client,
    cache: Arc<Mutex<Option<(Instant, JwkSet)>>>,
}
#[derive(Deserialize)]
struct Claims {
    sub: String,
    iat: u64,
}
impl Auth {
    pub fn new(app_id: Option<String>, client: reqwest::Client) -> Self {
        Self {
            app_id,
            client,
            cache: Arc::new(Mutex::new(None)),
        }
    }
    pub async fn verify(&self, headers: &HeaderMap) -> Result<String> {
        let app = self
            .app_id
            .as_deref()
            .ok_or_else(|| ApiError::unavailable("Account sign-in is being configured."))?;
        let token = bearer(headers)?;
        if token.len() > 8192 {
            return Err(ApiError::unauthorized(
                "Your sign-in expired. Please sign in again.",
            ));
        }
        let header =
            decode_header(token).map_err(|_| ApiError::unauthorized("Invalid sign-in token."))?;
        if header.alg != Algorithm::ES256 {
            return Err(ApiError::unauthorized("Invalid sign-in algorithm."));
        }
        let key_id = header
            .kid
            .as_deref()
            .ok_or_else(|| ApiError::unauthorized("Invalid sign-in token."))?;
        let mut cache = self.cache.lock().await;
        if cache.as_ref().is_none_or(|(at, keys)| {
            at.elapsed() > Duration::from_secs(300)
                || (keys.find(key_id).is_none() && at.elapsed() > Duration::from_secs(30))
        }) {
            let url = format!("https://auth.privy.io/api/v1/apps/{app}/jwks.json");
            let response = self
                .client
                .get(url)
                .timeout(Duration::from_secs(8))
                .send()
                .await
                .map_err(|_| {
                    ApiError::unavailable("Sign-in verification is temporarily unavailable.")
                })?;
            let keys = response
                .error_for_status()
                .map_err(|_| {
                    ApiError::unavailable("Sign-in verification is temporarily unavailable.")
                })?
                .json::<JwkSet>()
                .await
                .map_err(|_| {
                    ApiError::unavailable("Sign-in verification is temporarily unavailable.")
                })?;
            *cache = Some((Instant::now(), keys));
        }
        let jwk = cache
            .as_ref()
            .and_then(|(_, keys)| keys.find(key_id))
            .ok_or_else(|| ApiError::unauthorized("Invalid sign-in key."))?;
        let key = DecodingKey::from_jwk(jwk)
            .map_err(|_| ApiError::unauthorized("Invalid sign-in key."))?;
        let mut validation = Validation::new(Algorithm::ES256);
        validation.set_audience(&[app]);
        validation.set_issuer(&["privy.io"]);
        validation.required_spec_claims =
            HashSet::from_iter(["sub", "exp", "iat", "iss", "aud"].map(str::to_owned));
        validation.leeway = 30;
        let claims = decode::<Claims>(token, &key, &validation)
            .map_err(|_| ApiError::unauthorized("Your sign-in expired. Please sign in again."))?
            .claims;
        if !claims.sub.starts_with("did:privy:")
            || claims.sub.len() > 256
            || claims.iat > chrono::Utc::now().timestamp().max(0) as u64 + 30
        {
            return Err(ApiError::unauthorized(
                "Invalid account subject or issuance time.",
            ));
        }
        Ok(claims.sub)
    }
}
pub fn bearer(headers: &HeaderMap) -> Result<&str> {
    let value = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::unauthorized("Sign in to access your account."))?;
    let (scheme, token) = value
        .split_once(' ')
        .ok_or_else(|| ApiError::unauthorized("Use bearer authentication."))?;
    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
        return Err(ApiError::unauthorized("Use bearer authentication."));
    }
    Ok(token)
}
pub struct Owner(pub String);
impl FromRequestParts<AppState> for Owner {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        state.auth.verify(&parts.headers).await.map(Self)
    }
}
pub struct Builder(pub String, pub String);
impl FromRequestParts<AppState> for Builder {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        use rusqlite::OptionalExtension;
        use sha2::{Digest, Sha256};
        let token = bearer(&parts.headers)?;
        if token.len() > 128 {
            return Err(ApiError::unauthorized("Invalid agent access key."));
        }
        let hash = hex::encode(Sha256::digest(token.as_bytes()));
        let row:Option<(String,String)>=state.store.connect()?.query_row("SELECT a.owner,a.id FROM agent_keys k JOIN runtime_agents a ON a.id=k.agent_id WHERE k.hash=?",[hash],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        row.map(|(owner, id)| Self(owner, id))
            .ok_or_else(|| ApiError::unauthorized("Invalid agent access key."))
    }
}
