//! Holder access belongs to the app, not to the immutable, permissionless contracts.
//! A wallet signature binds an account to a wallet. Holding is checked afresh for
//! each new action; neither a saved signature nor a previous balance grants access.
use crate::{
    auth::{Builder, Owner},
    bnb,
    db::Store,
    error::{ApiError, Result},
    models::{identifier, now},
    AppState,
};
use axum::{
    body::{to_bytes, Body},
    extract::{FromRequestParts, Query, Request, State},
    http::Method,
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{Duration, Utc};
use ethabi::{ethereum_types::U256, Token};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::time::Duration as Timeout;
use utoipa::ToSchema;

pub fn enabled_from_environment() -> bool {
    // A malformed nonempty setting must not silently disable the restriction.
    std::env::var("TAB_HOLDER_ACCESS_ENABLED")
        .ok()
        .is_some_and(|v| !v.is_empty() && v != "false")
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HolderAccess {
    pub status: String,
    pub enforced: bool,
    pub eligible: bool,
    pub chain_id: u64,
    pub token_address: Option<String>,
    pub wallet: Option<String>,
    pub balance_units: Option<String>,
    pub minimum_units: String,
    pub decimals: Option<u8>,
    pub checked_at: Option<String>,
    pub message: String,
    pub recovery_allowed: bool,
}
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HolderChallenge {
    pub id: String,
    pub wallet: String,
    pub message: String,
    pub expires_at: String,
    pub chain_id: u64,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct WalletInput {
    pub wallet: String,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProofInput {
    pub id: String,
    pub signature: String,
}
#[derive(Deserialize)]
struct WalletQuery {
    wallet: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/account/holder-access", get(status))
        .route("/api/account/holder-access/challenge", post(challenge))
        .route("/api/account/holder-access/verify", post(verify))
}
async fn status(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Query(q): Query<WalletQuery>,
) -> Result<Json<HolderAccess>> {
    Ok(Json(s.holder_access(&owner, q.wallet.as_deref()).await?))
}
async fn challenge(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Json(input): Json<WalletInput>,
) -> Result<Json<HolderChallenge>> {
    Ok(Json(s.holder_challenge(&owner, &input.wallet)?))
}
async fn verify(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Json(input): Json<ProofInput>,
) -> Result<Json<HolderAccess>> {
    Ok(Json(s.verify_holder_wallet(&owner, input).await?))
}

fn schema(store: &Store) -> Result<()> {
    store.connect()?.execute_batch("CREATE TABLE IF NOT EXISTS holder_wallet_challenges(id TEXT PRIMARY KEY,owner TEXT NOT NULL,wallet TEXT NOT NULL,message TEXT NOT NULL,expires_at TEXT NOT NULL,used INTEGER NOT NULL DEFAULT 0); CREATE INDEX IF NOT EXISTS holder_challenge_owner ON holder_wallet_challenges(owner); CREATE TABLE IF NOT EXISTS holder_wallet_links(owner TEXT PRIMARY KEY,wallet TEXT NOT NULL,verified_at TEXT NOT NULL);")?;
    Ok(())
}
fn message(owner: &str, wallet: &str, nonce: &str, expires: &str) -> String {
    format!("tabagents.io\nBNB Smart Chain mainnet 56\nVerify TAB holder access\nAccount: {owner}\nWallet: {wallet}\nNonce: {nonce}\nExpires: {expires}\nThis signature proves wallet control for TAB holder access. It does not authorize a transaction.")
}
fn selector(signature: &str) -> Vec<u8> {
    Keccak256::digest(signature.as_bytes())[..4].to_vec()
}
fn decode_uint(value: Value) -> Result<U256> {
    let raw = value
        .as_str()
        .and_then(|v| v.strip_prefix("0x"))
        .ok_or_else(|| ApiError::unavailable("Invalid TAB token response."))?;
    if raw.len() != 64 {
        return Err(ApiError::unavailable("Invalid TAB token response."));
    }
    let bytes =
        hex::decode(raw).map_err(|_| ApiError::unavailable("Invalid TAB token response."))?;
    Ok(U256::from_big_endian(&bytes))
}

impl AppState {
    fn holder_result(&self, status: &str, wallet: Option<String>, message: &str) -> HolderAccess {
        HolderAccess {
            status: status.into(),
            enforced: self.holder_access_enabled,
            eligible: false,
            chain_id: 56,
            token_address: self.config.official_tab.clone(),
            wallet,
            balance_units: None,
            minimum_units: "1".into(),
            decimals: None,
            checked_at: None,
            message: message.into(),
            recovery_allowed: true,
        }
    }
    fn linked_holder_wallet(&self, owner: &str) -> Result<Option<String>> {
        schema(&self.store)?;
        Ok(self
            .store
            .connect()?
            .query_row(
                "SELECT wallet FROM holder_wallet_links WHERE owner=?",
                [owner],
                |r| r.get(0),
            )
            .optional()?)
    }
    pub fn holder_challenge(&self, owner: &str, wallet: &str) -> Result<HolderChallenge> {
        let wallet = bnb::address(wallet)?;
        if wallet == bnb::ZERO {
            return Err(ApiError::validation("Choose a nonzero wallet address."));
        }
        schema(&self.store)?;
        let mut db = self.store.connect()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let recent: Option<(String,String,String)> = tx.query_row("SELECT id,message,expires_at FROM holder_wallet_challenges WHERE owner=? AND wallet=? AND used=0 AND expires_at>? ORDER BY expires_at DESC LIMIT 1", params![owner,wallet,now()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        if let Some((id, message, expires_at)) = recent {
            return Ok(HolderChallenge {
                id,
                wallet,
                message,
                expires_at,
                chain_id: 56,
            });
        }
        // Only the latest challenge per account can establish the selected wallet.
        tx.execute(
            "DELETE FROM holder_wallet_challenges WHERE owner=?",
            [owner],
        )?;
        let id = identifier();
        let expires_at = (Utc::now() + Duration::minutes(5)).to_rfc3339();
        let message = message(owner, &wallet, &id, &expires_at);
        tx.execute(
            "INSERT INTO holder_wallet_challenges VALUES(?,?,?,?,?,0)",
            params![id, owner, wallet, message, expires_at],
        )?;
        tx.commit()?;
        Ok(HolderChallenge {
            id,
            wallet,
            message,
            expires_at,
            chain_id: 56,
        })
    }
    pub async fn verify_holder_wallet(
        &self,
        owner: &str,
        proof: ProofInput,
    ) -> Result<HolderAccess> {
        schema(&self.store)?;
        let row: Option<(String,String,String)> = self.store.connect()?.query_row("SELECT wallet,message,expires_at FROM holder_wallet_challenges WHERE id=? AND owner=? AND used=0",params![proof.id,owner],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let (wallet, message, expires) = row
            .ok_or_else(|| ApiError::conflict("Request a fresh wallet verification challenge."))?;
        if expires <= now() {
            return Err(ApiError::conflict(
                "Wallet verification expired. Request a fresh challenge.",
            ));
        }
        tokio::time::timeout(Timeout::from_secs(8),async {
            self.bnb.require_network().await?;
            let code = self.bnb.rpc("eth_getCode",json!([wallet,"latest"])).await?;
            if code == "0x" || code == "0x0" {
                if bnb::personal_signer(&message,&proof.signature)? != wallet { return Err(ApiError::forbidden("This signature belongs to another wallet.")); }
            } else {
                let bytes = hex::decode(proof.signature.strip_prefix("0x").unwrap_or("")).map_err(|_|ApiError::validation("Invalid wallet signature."))?;
                if bytes.is_empty() || bytes.len()>4096 { return Err(ApiError::validation("Invalid wallet signature length.")); }
                let digest=Keccak256::new().chain_update(format!("\x19Ethereum Signed Message:\n{}",message.len())).chain_update(message.as_bytes()).finalize();
                let mut data=selector("isValidSignature(bytes32,bytes)");
                data.extend(ethabi::encode(&[Token::FixedBytes(digest.to_vec()),Token::Bytes(bytes)]));
                let result=self.bnb.rpc("eth_call",json!([{"to":wallet,"data":format!("0x{}",hex::encode(data)),"gas":"0x186a0"},"latest"])).await?;
                if !result.as_str().is_some_and(|v|v.starts_with("0x1626ba7e")) { return Err(ApiError::forbidden("The wallet did not approve this signature.")); }
            }
            Ok::<_,ApiError>(())
        }).await.map_err(|_|ApiError::unavailable("Wallet verification is temporarily unavailable."))??;
        {
            let mut db = self.store.connect()?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            if tx.execute("UPDATE holder_wallet_challenges SET used=1 WHERE id=? AND owner=? AND used=0 AND expires_at>?",params![proof.id,owner,now()])? != 1 { return Err(ApiError::conflict("Wallet verification was already used or expired.")); }
            tx.execute("INSERT INTO holder_wallet_links VALUES(?,?,?) ON CONFLICT(owner) DO UPDATE SET wallet=excluded.wallet,verified_at=excluded.verified_at",params![owner,wallet,now()])?;
            tx.commit()?;
        }
        self.holder_access(owner, Some(&wallet)).await
    }
    async fn holder_wallet_is_owned(&self, owner: &str, wallet: &str) -> Result<bool> {
        if self.linked_holder_wallet(owner)?.as_deref() == Some(wallet) {
            return Ok(true);
        }
        for agent in self.store.list_agents(owner)? {
            if agent.wallet.as_deref() == Some(wallet)
                && agent.registration_tx.is_some()
                && agent.registry_id.is_some()
            {
                return Ok(self.verify_owned_agent(&agent).await? == wallet);
            }
        }
        Ok(false)
    }
    pub(crate) async fn verified_official_tab(&self) -> Result<(String, u8)> {
        let token =
            self.config.official_tab.as_deref().ok_or_else(|| {
                ApiError::unavailable("The official TAB token is not configured.")
            })?;
        let manifest = self.bnb.manifest()?;
        if token == bnb::ZERO
            || manifest["chain_id"] != 56
            || manifest["official_tab_address"] != token
            || manifest["contracts"]["protocol"]["address"] != self.config.program
        {
            return Err(ApiError::unavailable(
                "The official TAB deployment is not verified.",
            ));
        }
        let expected = manifest["official_tab_code_hash"].as_str().ok_or_else(|| {
            ApiError::unavailable("The official TAB token runtime is not pinned.")
        })?;
        self.bnb.require_network().await?;
        let code = self
            .bnb
            .rpc("eth_getCode", json!([token, "latest"]))
            .await?;
        let bytes = code
            .as_str()
            .and_then(|v| v.strip_prefix("0x"))
            .and_then(|v| hex::decode(v).ok())
            .filter(|b| !b.is_empty())
            .ok_or_else(|| ApiError::unavailable("The official TAB token has no verified code."))?;
        if bnb::hash(&bytes) != expected
            || self.bnb.view("protocol", "tabToken", vec![]).await? != token
        {
            return Err(ApiError::unavailable(
                "The official TAB token differs from the verified deployment.",
            ));
        }
        // EIP-1167 clones pin their implementation in the runtime. Checking only
        // the tiny proxy is insufficient evidence about the token logic.
        const PREFIX: &[u8] = &[0x36, 0x3d, 0x3d, 0x37, 0x3d, 0x3d, 0x3d, 0x36, 0x3d, 0x73];
        const SUFFIX: &[u8] = &[
            0x5a, 0xf4, 0x3d, 0x82, 0x80, 0x3e, 0x90, 0x3d, 0x91, 0x60, 0x2b, 0x57, 0xfd, 0x5b,
            0xf3,
        ];
        if bytes.len() == 45 && bytes.starts_with(PREFIX) && bytes.ends_with(SUFFIX) {
            let implementation = format!("0x{}", hex::encode(&bytes[10..30]));
            if manifest["official_tab_implementation_address"] != implementation {
                return Err(ApiError::unavailable(
                    "The official TAB implementation is not pinned.",
                ));
            }
            let code = self
                .bnb
                .rpc("eth_getCode", json!([implementation, "latest"]))
                .await?;
            let implementation_bytes = code
                .as_str()
                .and_then(|v| v.strip_prefix("0x"))
                .and_then(|v| hex::decode(v).ok())
                .filter(|b| !b.is_empty())
                .ok_or_else(|| {
                    ApiError::unavailable("The official TAB implementation has no code.")
                })?;
            if manifest["official_tab_implementation_code_hash"] != bnb::hash(&implementation_bytes)
            {
                return Err(ApiError::unavailable(
                    "The official TAB implementation changed.",
                ));
            }
        } else if !manifest["official_tab_implementation_address"].is_null()
            || !manifest["official_tab_implementation_code_hash"].is_null()
        {
            return Err(ApiError::unavailable(
                "The official TAB proxy differs from its pinned implementation.",
            ));
        }
        let decimals = self.bnb.token_decimals(token).await?;
        Ok((token.to_owned(), decimals))
    }
    async fn checked_holder_balance(&self, wallet: &str) -> Result<(U256, u8)> {
        let (token, decimals) = self.verified_official_tab().await?;
        let mut data = selector("balanceOf(address)");
        data.extend(ethabi::encode(&[bnb::addr(wallet)?]));
        let balance=decode_uint(self.bnb.rpc("eth_call",json!([{"to":token,"data":format!("0x{}",hex::encode(data)),"gas":"0x186a0"},"latest"])).await?)?;
        Ok((balance, decimals))
    }
    pub async fn holder_access(
        &self,
        owner: &str,
        requested_wallet: Option<&str>,
    ) -> Result<HolderAccess> {
        let wallet = requested_wallet
            .map(bnb::address)
            .transpose()?
            .or(self.linked_holder_wallet(owner)?);
        if !self.holder_access_enabled {
            return Ok(self.holder_result(
                "disabled",
                wallet,
                "TAB holder access is not currently enforced.",
            ));
        }
        if self.config.official_tab.is_none() {
            return Ok(self.holder_result("not_configured",wallet,"New actions require TAB. The official token is awaiting verification; existing funds remain recoverable."));
        }
        let Some(wallet) = wallet else {
            return Ok(self.holder_result(
                "wallet_required",
                None,
                "Connect and verify the wallet that holds TAB.",
            ));
        };
        let result=tokio::time::timeout(Timeout::from_secs(8),async {
            if !self.holder_wallet_is_owned(owner,&wallet).await? { return Ok(self.holder_result("wallet_unverified",Some(wallet.clone()),"Verify this wallet before checking TAB holder access.")); }
            let (balance,decimals)=self.checked_holder_balance(&wallet).await?;
            let mut result=self.holder_result(if balance.is_zero(){"not_holder"}else{"eligible"},Some(wallet.clone()),if balance.is_zero(){"This wallet needs a positive TAB balance for new actions. Repayments and withdrawals remain available."}else{"TAB holding verified. Feature readiness and spending limits still apply."});
            result.eligible=!balance.is_zero();result.balance_units=Some(balance.to_string());result.decimals=Some(decimals);result.checked_at=Some(now());Ok::<_,ApiError>(result)
        }).await;
        Ok(match result { Ok(Ok(value))=>value,_=>self.holder_result("unavailable",Some(wallet),"TAB holdings could not be verified. New actions are paused; repayments and withdrawals remain available.") })
    }
    async fn require_holder_wallet(&self, owner: &str, wallet: Option<&str>) -> Result<()> {
        if !self.holder_access_enabled {
            return Ok(());
        }
        let access = self.holder_access(owner, wallet).await?;
        if access.eligible {
            return Ok(());
        }
        Err(
            if ["unavailable", "not_configured"].contains(&access.status.as_str()) {
                ApiError::unavailable(access.message)
            } else {
                ApiError::forbidden(access.message)
            },
        )
    }
    pub async fn require_holder_agent(&self, owner: &str, id: &str) -> Result<()> {
        if !self.holder_access_enabled {
            return Ok(());
        }
        let agent = self.store.agent(owner, id)?;
        // The actual action wallet wins over a different linked wallet with TAB.
        self.require_holder_wallet(owner, agent.wallet.as_deref())
            .await
    }
}

/// Recovering an existing commitment must never depend on continuing to hold TAB.
fn recovery(method: &Method, path: &str, body: &Value) -> bool {
    if [Method::GET, Method::HEAD, Method::OPTIONS].contains(method) {
        return true;
    }
    let p: Vec<_> = path.trim_matches('/').split('/').collect();
    if *method == Method::DELETE {
        return matches!(
            p.as_slice(),
            ["api", "account", "agents", _] | ["api", "account", "runtime", _, "key"]
        );
    }
    if p == ["api", "account", "profile"] || p.get(2) == Some(&"holder-access") {
        return true;
    }
    match p.as_slice() {
        ["api", "account", "runtime", _, "register"] => true,
        ["api", "account", "runtime", _, "sponsor", "status"] => true,
        ["api", "account", "runtime", _, "purchase", _, "status"] => true,
        ["api", "account", "runtime", _, "x402", _, "reconcile" | "release-expired"] => true,
        ["api", "account", "runtime", _, "pause"] => body["paused"] == true,
        ["api", "account", "wallet-actions" | "job-actions", _, "submitted" | "confirm" | "release-failed"] => {
            true
        }
        ["api", "account", "jobs", _, "refresh" | "cancel-draft" | "evidence"] => true,
        ["api", "agent", "jobs", _, "evidence"] => true,
        ["api", "account", "jobs", _, "prepare"] => [
            "submit",
            "accept",
            "reject",
            "cancel",
            "close_branch",
            "pause",
        ]
        .contains(&body["action"].as_str().unwrap_or("")),
        ["api", "account", "runtime", _, "wallet-actions", "prepare"] => {
            [
                "withdraw_bnb",
                "withdraw_backing",
                "withdraw_spending",
                "unstake",
                "settle_bond",
                "resolve_outcome",
                "claim_outcome",
                "revoke_session",
                "repay_credit",
                "withdraw_credit",
                "close_credit",
                "pledge_collateral",
                "withdraw_collateral",
                "liquidate_credit",
            ]
            .contains(&body["action"].as_str().unwrap_or(""))
                || (body["action"] == "pause_agent" && body["paused"] == true)
        }
        ["api", "account", "runtime", _, "finance", "prepare"] => [
            "pool_redeem",
            "stock_redeem",
            "advance_repay",
            "advance_close",
            "advance_pledge",
            "advance_withdraw_collateral",
            "advance_liquidate",
            "stock_repay",
            "stock_add_collateral",
            "stock_withdraw",
            "stock_liquidate",
        ]
        .contains(&body["action"].as_str().unwrap_or("")),
        _ => false,
    }
}
async fn authorize(
    state: &AppState,
    owner: &str,
    builder: Option<&str>,
    path: &str,
    body: &Value,
    selected_wallet: Option<&str>,
) -> Result<()> {
    if let Some(id) = builder {
        return state.require_holder_agent(owner, id).await;
    }
    let p: Vec<_> = path.trim_matches('/').split('/').collect();
    match p.as_slice() {
        ["api", "account", "runtime", id, "challenge" | "sponsor"] => {
            state.store.agent(owner, id)?;
            let wallet = body["wallet"]
                .as_str()
                .ok_or_else(|| ApiError::validation("Choose the registration wallet."))?;
            state.require_holder_wallet(owner, Some(wallet)).await
        }
        ["api", "account", "runtime", id, ..] => state.require_holder_agent(owner, id).await,
        ["api", "account", "jobs", id, "branches" | "run"] => {
            let job = state.get_job(owner, id)?;
            state
                .require_holder_agent(owner, &job.plan.executor_id)
                .await
        }
        ["api", "account", "jobs", id, "prepare"] => {
            let job = state.get_job(owner, id)?;
            let action = body["action"].as_str().unwrap_or("");
            let actor = if action == "resume" {
                state.get_job(owner, &job.root_id)?.requester_id
            } else if ["fund", "delegate"].contains(&action) {
                if let Some(parent) = job.parent_id {
                    state.get_job(owner, &parent)?.plan.executor_id
                } else {
                    job.requester_id
                }
            } else {
                job.plan.executor_id
            };
            state.require_holder_agent(owner, &actor).await
        }
        ["api", "account", "runtime" | "agents" | "bounties"] => {
            state.require_holder_wallet(owner, selected_wallet).await
        }
        _ => Err(ApiError::forbidden(
            "This new action has no verified holder wallet binding.",
        )),
    }
}
pub async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    if !state.holder_access_enabled
        || !(path.starts_with("/api/account/") || path.starts_with("/api/agent/"))
        || [Method::GET, Method::HEAD, Method::OPTIONS].contains(request.method())
    {
        return next.run(request).await;
    }
    let (mut parts, body) = request.into_parts();
    let bytes = match to_bytes(body, 32_768).await {
        Ok(b) => b,
        Err(_) => {
            return ApiError::bad("Request body exceeds the supported limit.").into_response()
        }
    };
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => return ApiError::bad("Use a valid JSON request.").into_response(),
        }
    };
    if !recovery(&parts.method, &path, &body) {
        // A header is only a wallet selection, never evidence of ownership.
        // Agent and job actions always derive their wallet from owned records.
        let selected_wallet = if parts.method == Method::POST
            && [
                "/api/account/agents",
                "/api/account/runtime",
                "/api/account/bounties",
            ]
            .contains(&path.as_str())
        {
            match parts
                .headers
                .get("x-tab-holder-wallet")
                .map(|v| v.to_str())
                .transpose()
            {
                Ok(value) => value.map(str::to_owned),
                Err(_) => {
                    return ApiError::validation("Use a valid holder wallet header.")
                        .into_response()
                }
            }
        } else {
            None
        };
        let identity = if path.starts_with("/api/agent/") {
            Builder::from_request_parts(&mut parts, &state)
                .await
                .map(|Builder(owner, id)| (owner, Some(id)))
        } else {
            Owner::from_request_parts(&mut parts, &state)
                .await
                .map(|Owner(owner)| (owner, None))
        };
        let result = match identity {
            Ok((owner, id)) => {
                authorize(
                    &state,
                    &owner,
                    id.as_deref(),
                    &path,
                    &body,
                    selected_wallet.as_deref(),
                )
                .await
            }
            Err(e) => Err(e),
        };
        if let Err(error) = result {
            return error.into_response();
        }
    }
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Request as HttpRequest, StatusCode};
    use http_body_util::BodyExt;
    use k256::ecdsa::SigningKey;
    use std::sync::Arc;
    use tower::ServiceExt;
    const WALLET: &str = "0x7e5f4552091a69125d5dfcb7b8c2659029395bdf";
    const OTHER: &str = "0x2222222222222222222222222222222222222222";
    const TAB: &str = "0x8888888888888888888888888888888888888888";
    fn encoded(token: Token) -> Value {
        json!(format!("0x{}", hex::encode(ethabi::encode(&[token]))))
    }
    fn call(signature: &str) -> String {
        format!("call:0x{}", hex::encode(selector(signature)))
    }
    async fn fixture() -> crate::chain_tests::Fixture {
        let mut f = crate::chain_tests::fixture().await;
        f.state.holder_access_enabled = true;
        let config = Arc::make_mut(&mut f.state.config);
        config.official_tab = Some(TAB.into());
        let mut manifest = f.state.bnb.manifest().unwrap();
        manifest["official_tab_address"] = json!(TAB);
        manifest["official_tab_code_hash"] = json!(bnb::hash(&hex::decode("60006000").unwrap()));
        std::fs::write(
            &f.state.config.manifest,
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        f.state.bnb = bnb::Bnb::new(f.state.config.clone(), f.state.client.clone());
        let mut replies = f.replies.lock().unwrap();
        replies.insert(call("tabToken()"), encoded(bnb::addr(TAB).unwrap()));
        replies.insert(call("balanceOf(address)"), encoded(bnb::uint(1)));
        replies.insert(format!("code:{WALLET}"), json!("0x"));
        drop(replies);
        f
    }
    fn link(state: &AppState, owner: &str, wallet: &str) {
        schema(&state.store).unwrap();
        state
            .store
            .connect()
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO holder_wallet_links VALUES(?,?,?)",
                params![owner, wallet, now()],
            )
            .unwrap();
    }
    fn sign(text: &str) -> String {
        let mut raw = [0u8; 32];
        raw[31] = 1;
        let key = SigningKey::from_slice(&raw).unwrap();
        let digest = Keccak256::new()
            .chain_update(format!("\x19Ethereum Signed Message:\n{}", text.len()))
            .chain_update(text.as_bytes())
            .finalize();
        let (sig, recovery) = key.sign_prehash_recoverable(&digest).unwrap();
        let mut bytes = sig.to_bytes().to_vec();
        bytes.push(recovery.to_byte() + 27);
        format!("0x{}", hex::encode(bytes))
    }
    fn agent(state: &AppState, owner: &str, wallet: Option<&str>) -> crate::models::RuntimeAgent {
        let mut agent=state.create_agent(owner,serde_json::from_value(json!({"name":"holder test","purpose":"check current onchain data","tools":["bnb-rpc"],"daily_cap":"1","max_call":"0.1"})).unwrap()).unwrap();
        agent.wallet = wallet.map(str::to_owned);
        agent.status = "ready".into();
        agent.next_run = Some("2000-01-01T00:00:00+00:00".into());
        state.store.save_agent(&agent).unwrap();
        agent
    }
    #[tokio::test]
    async fn disabled_is_not_evidence_and_enabled_missing_token_fails_closed() {
        let (_dir, mut state) = crate::tests::state();
        let disabled = state.holder_access("alice", Some(WALLET)).await.unwrap();
        assert_eq!(disabled.status, "disabled");
        assert!(!disabled.enforced && !disabled.eligible);
        assert!(disabled.balance_units.is_none());
        state.holder_access_enabled = true;
        let blocked = state.holder_access("alice", Some(WALLET)).await.unwrap();
        assert_eq!(blocked.status, "not_configured");
        assert!(blocked.enforced && blocked.recovery_allowed && !blocked.eligible);
        assert_eq!(
            state
                .require_holder_wallet("alice", Some(WALLET))
                .await
                .unwrap_err()
                .0,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
    #[tokio::test]
    async fn positive_base_unit_qualifies_and_sold_balance_is_never_cached() {
        let f = fixture().await;
        link(&f.state, "alice", WALLET);
        let first = f.state.holder_access("alice", Some(WALLET)).await.unwrap();
        assert!(first.eligible);
        assert_eq!(first.balance_units.as_deref(), Some("1"));
        assert_eq!(first.minimum_units, "1");
        f.replies
            .lock()
            .unwrap()
            .insert(call("balanceOf(address)"), encoded(bnb::uint(0)));
        let sold = f.state.holder_access("alice", Some(WALLET)).await.unwrap();
        assert_eq!(sold.status, "not_holder");
        assert!(!sold.eligible);
        assert_eq!(
            f.state
                .require_holder_wallet("alice", Some(WALLET))
                .await
                .unwrap_err()
                .0,
            StatusCode::FORBIDDEN
        );
        f.replies
            .lock()
            .unwrap()
            .insert(call("balanceOf(address)"), encoded(Token::Uint(U256::MAX)));
        assert_eq!(
            f.state
                .holder_access("alice", Some(WALLET))
                .await
                .unwrap()
                .balance_units
                .unwrap(),
            U256::MAX.to_string()
        );
    }
    #[tokio::test]
    async fn wrong_chain_changed_code_and_rpc_failure_never_reuse_eligibility() {
        let f = fixture().await;
        link(&f.state, "alice", WALLET);
        assert!(f.state.holder_access("alice", None).await.unwrap().eligible);
        for (key, value) in [
            ("eth_chainId".to_owned(), json!("0x1")),
            (format!("code:{TAB}"), json!("0x1234")),
            (call("balanceOf(address)"), json!("0x01")),
        ] {
            let previous = f.replies.lock().unwrap().insert(key.clone(), value);
            assert_eq!(
                f.state.holder_access("alice", None).await.unwrap().status,
                "unavailable"
            );
            let mut replies = f.replies.lock().unwrap();
            if let Some(value) = previous {
                replies.insert(key, value);
            } else {
                replies.remove(&key);
            }
        }
        f.replies
            .lock()
            .unwrap()
            .remove(&call("balanceOf(address)"));
        assert_eq!(
            f.state.holder_access("alice", None).await.unwrap().status,
            "unavailable"
        );
    }
    #[tokio::test]
    async fn minimal_proxy_requires_matching_implementation_and_public_flag_is_observable() {
        let f = fixture().await;
        link(&f.state, "alice", WALLET);
        let implementation = "0x46862924e2a229170ebd065e24a0da72af58a986";
        let code = format!(
            "0x363d3d373d3d3d363d73{}5af43d82803e903d91602b57fd5bf3",
            &implementation[2..]
        );
        let mut manifest = f.state.bnb.manifest().unwrap();
        manifest["official_tab_code_hash"] = json!(bnb::hash(&hex::decode(&code[2..]).unwrap()));
        std::fs::write(
            &f.state.config.manifest,
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        f.replies
            .lock()
            .unwrap()
            .insert(format!("code:{TAB}"), json!(code));
        assert_eq!(
            f.state.holder_access("alice", None).await.unwrap().status,
            "unavailable"
        );
        f.replies.lock().unwrap().insert(
            call("getProtocol()"),
            encoded(Token::Tuple(vec![
                bnb::addr("0x4444444444444444444444444444444444444444").unwrap(),
                bnb::addr(bnb::USDT).unwrap(),
                bnb::addr(TAB).unwrap(),
                bnb::addr("0x2222222222222222222222222222222222222222").unwrap(),
                bnb::addr("0x3333333333333333333333333333333333333333").unwrap(),
                bnb::uint(200),
                bnb::uint(0),
                bnb::uint(0),
                bnb::uint(0),
                bnb::uint(0),
            ])),
        );
        assert_eq!(f.state.token_system().await["staking_enabled"], false);
        manifest["official_tab_implementation_address"] = json!(implementation);
        manifest["official_tab_implementation_code_hash"] =
            json!(bnb::hash(&hex::decode("60006000").unwrap()));
        std::fs::write(
            &f.state.config.manifest,
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(f.state.holder_access("alice", None).await.unwrap().eligible);
        assert_eq!(f.state.token_system().await["staking_enabled"], true);
        f.replies
            .lock()
            .unwrap()
            .insert(format!("code:{implementation}"), json!("0x6001"));
        assert_eq!(
            f.state.holder_access("alice", None).await.unwrap().status,
            "unavailable"
        );
        assert_eq!(f.state.token_system().await["staking_enabled"], false);
        for enabled in [true, false] {
            let mut state = f.state.clone();
            state.holder_access_enabled = enabled;
            let response = crate::api::router(state)
                .oneshot(
                    HttpRequest::builder()
                        .uri("/api/config")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap()["holder_access_enabled"],
                enabled
            );
        }
    }
    #[tokio::test]
    async fn wallet_link_rejects_spoofed_account_wallet_expiry_and_replay() {
        let f = fixture().await;
        assert_eq!(
            f.state
                .holder_access("alice", Some(WALLET))
                .await
                .unwrap()
                .status,
            "wallet_unverified"
        );
        let c = f.state.holder_challenge("alice", WALLET).unwrap();
        let signature = sign(&c.message);
        assert!(c.message.contains("Account: alice\n"));
        assert!(f
            .state
            .verify_holder_wallet(
                "mallory",
                ProofInput {
                    id: c.id.clone(),
                    signature: signature.clone()
                }
            )
            .await
            .is_err());
        assert!(f
            .state
            .verify_holder_wallet(
                "alice",
                ProofInput {
                    id: c.id.clone(),
                    signature: sign("different message")
                }
            )
            .await
            .is_err());
        assert!(
            f.state
                .verify_holder_wallet(
                    "alice",
                    ProofInput {
                        id: c.id.clone(),
                        signature: signature.clone()
                    }
                )
                .await
                .unwrap()
                .eligible
        );
        assert!(f
            .state
            .verify_holder_wallet(
                "alice",
                ProofInput {
                    id: c.id,
                    signature
                }
            )
            .await
            .is_err());
        assert_eq!(
            f.state
                .holder_access("mallory", Some(WALLET))
                .await
                .unwrap()
                .status,
            "wallet_unverified"
        );
        let stale = f.state.holder_challenge("alice", WALLET).unwrap();
        f.state
            .store
            .connect()
            .unwrap()
            .execute(
                "UPDATE holder_wallet_challenges SET expires_at='2000-01-01' WHERE id=?",
                [&stale.id],
            )
            .unwrap();
        assert!(f
            .state
            .verify_holder_wallet(
                "alice",
                ProofInput {
                    id: stale.id,
                    signature: sign(&stale.message)
                }
            )
            .await
            .is_err());
        let other = f.state.holder_challenge("alice", OTHER).unwrap();
        f.replies
            .lock()
            .unwrap()
            .insert(format!("code:{OTHER}"), json!("0x"));
        assert!(f
            .state
            .verify_holder_wallet(
                "alice",
                ProofInput {
                    id: other.id,
                    signature: sign(&other.message)
                }
            )
            .await
            .is_err());
    }
    #[tokio::test]
    async fn contract_wallet_proof_is_bounded_and_challenges_are_single_use() {
        let f = fixture().await;
        let c = f.state.holder_challenge("alice", OTHER).unwrap();
        f.replies.lock().unwrap().insert(
            call("isValidSignature(bytes32,bytes)"),
            json!(format!("0x1626ba7e{}", "00".repeat(28))),
        );
        let access = f
            .state
            .verify_holder_wallet(
                "alice",
                ProofInput {
                    id: c.id.clone(),
                    signature: "0x1234".into(),
                },
            )
            .await
            .unwrap();
        assert!(access.eligible);
        let r = f.replies.lock().unwrap()
            [&format!("request:{}", call("isValidSignature(bytes32,bytes)"))]
            .clone();
        assert_eq!(r["params"][0]["gas"], "0x186a0");
        assert!(f
            .state
            .verify_holder_wallet(
                "alice",
                ProofInput {
                    id: c.id,
                    signature: "0x1234".into()
                }
            )
            .await
            .is_err());
    }
    #[tokio::test]
    async fn selected_holder_cannot_unlock_another_action_wallet_or_owner() {
        let f = fixture().await;
        link(&f.state, "alice", WALLET);
        let a = agent(&f.state, "alice", Some(OTHER));
        assert!(f.state.require_holder_agent("alice", &a.id).await.is_err());
        assert!(f
            .state
            .require_holder_agent("mallory", &a.id)
            .await
            .is_err());
        assert!(authorize(
            &f.state,
            "alice",
            None,
            &format!("/api/account/runtime/{}/sponsor", a.id),
            &json!({"wallet":OTHER}),
            Some(WALLET)
        )
        .await
        .is_err());
        let buyer = agent(&f.state, "alice", Some(WALLET));
        let job = f.state.create_job("alice", &buyer.id, serde_json::from_value(json!({"title":"holder binding test","description":"run against the exact executor wallet","executor_id":a.id,"budget":"1","max_call":"0.1","deadline":(Utc::now()+Duration::hours(1)).to_rfc3339(),"tools":["bnb-rpc"]})).unwrap(), None).await.unwrap();
        assert!(authorize(
            &f.state,
            "alice",
            None,
            &format!("/api/account/jobs/{}/run", job.id),
            &Value::Null,
            Some(WALLET)
        )
        .await
        .is_err());
        assert!(authorize(
            &f.state,
            "alice",
            None,
            "/api/account/future-new-feature",
            &Value::Null,
            Some(WALLET)
        )
        .await
        .is_err());
    }
    #[tokio::test]
    async fn creation_selection_requires_owned_wallet_and_cannot_override_action_wallet() {
        let f = fixture().await;
        let mut registered = agent(&f.state, "alice", Some(WALLET));
        registered.registry_id = Some(f.state.bnb.agent_address(WALLET, &registered.id).unwrap());
        registered.registration_tx = Some(format!("0x{}", "aa".repeat(32)));
        f.state.store.save_agent(&registered).unwrap();
        f.replies.lock().unwrap().insert(
            call("getAgent(bytes32)"),
            encoded(Token::Tuple(vec![
                bnb::addr(WALLET).unwrap(),
                Token::String("holder test".into()),
                bnb::uint(1),
                Token::FixedBytes(vec![1; 32]),
                Token::Bool(false),
                bnb::uint(1),
                bnb::uint(0),
                bnb::uint(0),
            ])),
        );
        assert!(f.state.linked_holder_wallet("alice").unwrap().is_none());
        assert!(
            f.state
                .holder_access("alice", Some(WALLET))
                .await
                .unwrap()
                .eligible
        );
        for path in [
            "/api/account/runtime",
            "/api/account/agents",
            "/api/account/bounties",
        ] {
            assert!(authorize(&f.state, "alice", None, path, &Value::Null, None)
                .await
                .is_err());
            assert!(
                authorize(&f.state, "alice", None, path, &Value::Null, Some(WALLET))
                    .await
                    .is_ok()
            );
            assert!(
                authorize(&f.state, "alice", None, path, &Value::Null, Some(OTHER))
                    .await
                    .is_err()
            );
            assert!(
                authorize(&f.state, "mallory", None, path, &Value::Null, Some(WALLET))
                    .await
                    .is_err()
            );
        }
        let other = agent(&f.state, "alice", Some(OTHER));
        assert!(authorize(
            &f.state,
            "alice",
            None,
            &format!("/api/account/runtime/{}/wallet-actions/prepare", other.id),
            &json!({"action":"fund_spending"}),
            Some(WALLET)
        )
        .await
        .is_err());
        f.replies
            .lock()
            .unwrap()
            .insert(call("balanceOf(address)"), encoded(bnb::uint(0)));
        assert!(authorize(
            &f.state,
            "alice",
            None,
            "/api/account/runtime",
            &Value::Null,
            Some(WALLET)
        )
        .await
        .is_err());
    }
    #[test]
    fn recovery_allowlist_preserves_funds_without_allowing_resume_or_new_risk() {
        for action in [
            "withdraw_spending",
            "repay_credit",
            "withdraw_credit",
            "close_credit",
            "revoke_session",
            "withdraw_collateral",
            "liquidate_credit",
        ] {
            assert!(recovery(
                &Method::POST,
                "/api/account/runtime/a/wallet-actions/prepare",
                &json!({"action":action})
            ));
        }
        for action in [
            "fund_spending",
            "open_credit",
            "accept_credit",
            "spend_credit",
            "grant_session",
            "deploy_token",
            "fund_bounty",
        ] {
            assert!(!recovery(
                &Method::POST,
                "/api/account/runtime/a/wallet-actions/prepare",
                &json!({"action":action})
            ));
        }
        assert!(recovery(
            &Method::POST,
            "/api/account/runtime/a/wallet-actions/prepare",
            &json!({"action":"pause_agent","paused":true})
        ));
        assert!(!recovery(
            &Method::POST,
            "/api/account/runtime/a/wallet-actions/prepare",
            &json!({"action":"pause_agent","paused":false})
        ));
        assert!(recovery(
            &Method::POST,
            "/api/account/runtime/a/pause",
            &json!({"paused":true})
        ));
        assert!(!recovery(
            &Method::POST,
            "/api/account/runtime/a/pause",
            &json!({"paused":false})
        ));
        for action in [
            "pool_redeem",
            "stock_redeem",
            "advance_repay",
            "advance_pledge",
            "advance_withdraw_collateral",
            "advance_liquidate",
            "stock_repay",
            "stock_withdraw",
        ] {
            assert!(recovery(
                &Method::POST,
                "/api/account/runtime/a/finance/prepare",
                &json!({"action":action})
            ));
        }
        for action in [
            "pool_deposit",
            "stock_borrow",
            "advance_spend",
            "advance_approve",
            "buyback_execute",
        ] {
            assert!(!recovery(
                &Method::POST,
                "/api/account/runtime/a/finance/prepare",
                &json!({"action":action})
            ));
        }
        for path in [
            "/api/account/runtime/a/register",
            "/api/account/runtime/a/sponsor/status",
            "/api/account/wallet-actions/a/confirm",
            "/api/account/wallet-actions/a/submitted",
            "/api/account/runtime/a/x402/q/reconcile",
        ] {
            assert!(recovery(&Method::POST, path, &Value::Null));
        }
        assert!(!recovery(
            &Method::POST,
            "/api/account/future-new-feature",
            &Value::Null
        ));
        assert!(!recovery(
            &Method::DELETE,
            "/api/account/future-new-feature",
            &Value::Null
        ));
    }
    #[tokio::test]
    async fn scheduled_and_builder_runs_cannot_bypass_enabled_gate() {
        let (_dir, mut state) = crate::tests::state();
        state.holder_access_enabled = true;
        let a = agent(&state, "alice", Some(WALLET));
        assert_eq!(
            state.run_agent("alice", &a.id).await.err().unwrap().0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        state.scheduled().await.unwrap();
        assert!(state.store.runs(&a.id).unwrap().is_empty());
        let key = state.create_key("alice", &a.id).unwrap();
        let response = crate::api::router(state)
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/agent/run")
                    .header("Authorization", format!("Bearer {key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    #[tokio::test]
    async fn access_and_recovery_routes_keep_normal_authentication_and_no_store() {
        let (_dir, mut state) = crate::tests::state();
        state.holder_access_enabled = true;
        for (method, path, body) in [
            ("GET", "/api/account/holder-access", Value::Null),
            (
                "POST",
                "/api/account/holder-access/challenge",
                json!({"wallet":WALLET}),
            ),
            (
                "POST",
                "/api/account/holder-access/verify",
                json!({"id":"missing","signature":"0x00"}),
            ),
            (
                "POST",
                "/api/account/wallet-actions/missing/confirm",
                json!({"tx_hash":"0x00"}),
            ),
        ] {
            let response = crate::api::router(state.clone())
                .oneshot(
                    HttpRequest::builder()
                        .method(method)
                        .uri(path)
                        .header("content-type", "application/json")
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            // This fixture has no auth app, so ordinary auth is unavailable; it
            // must never pass unauthenticated or fail with a holder decision.
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(response.headers()["cache-control"], "no-store");
            let body = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap()["detail"],
                "Account sign-in is being configured."
            );
        }
        let public = crate::api::router(state)
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/account/holder-access?wallet=bad")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(public.status(), StatusCode::OK);
    }
}
