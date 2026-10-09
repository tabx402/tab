//! Bounded registration-only gas sponsorship. Persist signed bytes before any broadcast.
//! Failed/unknown receipts never free a nonce or budget until canonical finality is proven.
use crate::{
    bnb::{self, Instruction},
    db::{decode, encode, Store},
    error::{ApiError, Result},
    models::*,
    AppState,
};
use chrono::{Duration, Utc};
use ethabi::Token;
use k256::ecdsa::SigningKey;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};

const PER_ACCOUNT_DAY: u64 = 3;
const PER_WALLET_DAY: u64 = 3;
const GLOBAL_DAY: u64 = 100;
const DAILY_WEI: u64 = 1_000_000_000_000_000;
const MAX_TX_WEI: u64 = 100_000_000_000_000;
const RESERVE_WEI: u64 = 50_000_000_000_000;
const MAX_GAS: u64 = 500_000;
const MAX_GAS_PRICE: u64 = 1_000_000_000;
const REGISTER_TYPE: &str = "Register(bytes32 id,address owner,string name,uint256 dailyCap,bytes32 policyHash,uint256 nonce,uint256 deadline)";

fn unavailable() -> ApiError {
    ApiError::unavailable(
        "Registration sponsorship is temporarily unavailable. No transaction was prepared.",
    )
}
fn limited(message: &str) -> ApiError {
    ApiError(axum::http::StatusCode::TOO_MANY_REQUESTS, message.into())
}
fn n(value: &Value) -> Result<u64> {
    bnb::number(value)?.try_into().map_err(|_| unavailable())
}
fn wei(value: u64) -> String {
    rust_decimal::Decimal::from_i128_with_scale(value as i128, 18)
        .normalize()
        .to_string()
}
fn signer(config: &crate::config::Config) -> Result<(SigningKey, String)> {
    let key = config.sponsor_key.as_deref().ok_or_else(unavailable)?;
    let bytes = hex::decode(key.strip_prefix("0x").unwrap_or(key)).map_err(|_| unavailable())?;
    let key = SigningKey::from_slice(&bytes).map_err(|_| unavailable())?;
    let point = key.verifying_key().to_encoded_point(false);
    let address = format!(
        "0x{}",
        hex::encode(&Keccak256::digest(&point.as_bytes()[1..])[12..])
    );
    if config.sponsor_address.as_deref() != Some(&address) {
        return Err(unavailable());
    }
    Ok((key, address))
}
#[derive(Clone, Serialize, Deserialize)]
struct Terms {
    request_id: String,
    agent_id: String,
    wallet: String,
    registry: String,
    registry_id: String,
    name: String,
    cap: String,
    policy: String,
    nonce: u64,
    deadline: u64,
}
impl Terms {
    fn typed(&self) -> Value {
        json!({"domain":{"name":"Tab Protocol","version":"1","chainId":56,"verifyingContract":self.registry},"primaryType":"Register","types":{"EIP712Domain":[{"name":"name","type":"string"},{"name":"version","type":"string"},{"name":"chainId","type":"uint256"},{"name":"verifyingContract","type":"address"}],"Register":[{"name":"id","type":"bytes32"},{"name":"owner","type":"address"},{"name":"name","type":"string"},{"name":"dailyCap","type":"uint256"},{"name":"policyHash","type":"bytes32"},{"name":"nonce","type":"uint256"},{"name":"deadline","type":"uint256"}]},"message":{"id":self.registry_id,"owner":self.wallet,"name":self.name,"dailyCap":self.cap,"policyHash":self.policy,"nonce":self.nonce.to_string(),"deadline":self.deadline.to_string()}})
    }
    fn digest(&self) -> Result<Vec<u8>> {
        let h = |v: &str| Token::FixedBytes(Keccak256::digest(v.as_bytes()).to_vec());
        let domain=Keccak256::digest(ethabi::encode(&[h("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"),h("Tab Protocol"),h("1"),bnb::uint(56),bnb::addr(&self.registry)?]));
        let cap = self.cap.parse::<u128>().map_err(|_| unavailable())?;
        let data = Keccak256::digest(ethabi::encode(&[
            h(REGISTER_TYPE),
            bnb::bytes32(&self.registry_id)?,
            bnb::addr(&self.wallet)?,
            h(&self.name),
            bnb::uint(cap),
            bnb::bytes32(&self.policy)?,
            bnb::uint(self.nonce.into()),
            bnb::uint(self.deadline.into()),
        ]));
        Ok(Keccak256::new()
            .chain_update([0x19, 0x01])
            .chain_update(domain)
            .chain_update(data)
            .finalize()
            .to_vec())
    }
    fn challenge(&self) -> Result<SponsoredChallenge> {
        Ok(SponsoredChallenge {
            sponsored: true,
            request_id: self.request_id.clone(),
            wallet: self.wallet.clone(),
            registry_id: self.registry_id.clone(),
            registry: self.registry.clone(),
            policy_hash: self.policy.clone(),
            chain_id: 56,
            expires_at: chrono::DateTime::from_timestamp(self.deadline as i64, 0)
                .ok_or_else(unavailable)?
                .to_rfc3339(),
            typed_data: self.typed(),
        })
    }
    fn instruction(&self, state: &AppState, signature: &str) -> Result<Instruction> {
        let bytes = hex::decode(signature.strip_prefix("0x").unwrap_or(""))
            .map_err(|_| ApiError::bad("Invalid registration signature."))?;
        if bytes.is_empty() || bytes.len() > 4096 {
            return Err(ApiError::bad("Invalid registration signature."));
        }
        state.bnb.instruction(
            "protocol",
            "registerWithSignature",
            vec![
                bnb::bytes32(&self.registry_id)?,
                bnb::addr(&self.wallet)?,
                Token::String(self.name.clone()),
                bnb::uint(self.cap.parse().map_err(|_| unavailable())?),
                bnb::bytes32(&self.policy)?,
                bnb::uint(self.nonce.into()),
                bnb::uint(self.deadline.into()),
                Token::Bytes(bytes),
            ],
        )
    }
    fn matches(&self, state: &AppState, agent: &RuntimeAgent) -> Result<bool> {
        Ok(self.agent_id == agent.id
            && self.name == agent.plan.name
            && self.cap == units(agent.plan.daily_cap)?.to_string()
            && self.policy == state.policy(agent)
            && self.registry == state.config.program
            && self.registry_id == state.bnb.agent_address(&self.wallet, &agent.id)?)
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Job {
    terms: Terms,
    instruction: Instruction,
    sponsor: String,
    nonce: u64,
    gas: u64,
    gas_price: u64,
    raw_tx: String,
    tx_hash: String,
}
struct Saved {
    job: Job,
    status: String,
    owner: String,
}
fn current_job(db: &Connection, owner: &str, agent: &str) -> Result<Option<Saved>> {
    let row:Option<(String,String)>=db.query_row("SELECT payload,status FROM sponsor_jobs WHERE owner=? AND agent_id=? ORDER BY rowid DESC LIMIT 1",params![owner,agent],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    row.map(|(payload, status)| {
        Ok(Saved {
            job: decode(payload)?,
            status,
            owner: owner.into(),
        })
    })
    .transpose()
}
fn saved_job(db: &Connection, owner: &str, request_id: &str) -> Result<Saved> {
    let (raw, status): (String, String) = db.query_row(
        "SELECT payload,status FROM sponsor_jobs WHERE owner=? AND request_id=?",
        params![owner, request_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(Saved {
        job: decode(raw)?,
        status,
        owner: owner.into(),
    })
}
pub(crate) fn ensure_unsigned(db: &Connection, id: &str) -> Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sponsor_jobs WHERE agent_id=? AND status='pending')",
        [id],
        |r| r.get(0),
    )?;
    if exists {
        Err(ApiError::conflict("Registration is already submitted. Check its sponsored confirmation before changing this setup."))
    } else {
        Ok(())
    }
}
fn budget(db: &Connection) -> Result<(u64, u64, u64)> {
    let day = Utc::now().format("%Y-%m-%d").to_string();
    let mut query = db
        .prepare("SELECT created_at,status,reserved_wei,actual_wei,settled_at FROM sponsor_jobs")?;
    let rows = query.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, u64>(2)?,
            r.get::<_, Option<u64>>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })?;
    let (mut count, mut used, mut pending) = (0u64, 0u64, 0u64);
    for row in rows {
        let (created, status, reserved, actual, settled) = row?;
        if created.starts_with(&day) {
            count += 1;
        }
        if status == "pending" {
            pending = pending.checked_add(reserved).ok_or_else(unavailable)?;
            used = used.checked_add(reserved).ok_or_else(unavailable)?;
        } else if settled.as_deref().unwrap_or(&created).starts_with(&day) {
            used = used
                .checked_add(actual.ok_or_else(unavailable)?)
                .ok_or_else(unavailable)?;
        }
    }
    Ok((count, used, pending))
}
fn quota(db: &Connection, owner: &str, wallet: &str, reserve: u64, balance: u64) -> Result<()> {
    let day = Utc::now().format("%Y-%m-%d").to_string();
    let counts:(u64,u64)=db.query_row("SELECT coalesce(sum(owner=?),0),coalesce(sum(wallet=?),0) FROM sponsor_jobs WHERE substr(created_at,1,10)=?",params![owner,wallet,day],|r|Ok((r.get(0)?,r.get(1)?)))?;
    if counts.0 >= PER_ACCOUNT_DAY || counts.1 >= PER_WALLET_DAY {
        return Err(limited(
            "Tab covers up to 3 registrations per account and wallet each day. Try again tomorrow.",
        ));
    }
    let (count, used, pending) = budget(db)?;
    if count >= GLOBAL_DAY || used.checked_add(reserve).is_none_or(|v| v > DAILY_WEI) {
        return Err(limited(
            "Today's registration sponsorship budget is reached. Try again tomorrow.",
        ));
    }
    if balance
        < pending
            .checked_add(reserve)
            .and_then(|v| v.checked_add(RESERVE_WEI))
            .ok_or_else(unavailable)?
    {
        return Err(ApiError::unavailable("The registration sponsor needs a BNB top-up. No registration transaction was submitted."));
    }
    Ok(())
}
fn signed_transaction(
    key: &SigningKey,
    nonce: u64,
    gas: u64,
    price: u64,
    ix: &Instruction,
) -> Result<(String, String)> {
    if ix.value != "0x0"
        || gas == 0
        || gas > MAX_GAS
        || price == 0
        || price > MAX_GAS_PRICE
        || gas.checked_mul(price).is_none_or(|v| v > MAX_TX_WEI)
    {
        return Err(unavailable());
    }
    let to =
        hex::decode(bnb::address(&ix.to)?.trim_start_matches("0x")).map_err(|_| unavailable())?;
    let data = hex::decode(ix.data.trim_start_matches("0x")).map_err(|_| unavailable())?;
    let prefix = |s: &mut rlp::RlpStream| {
        s.append(&nonce);
        s.append(&price);
        s.append(&gas);
        s.append(&to.as_slice());
        s.append(&0u8);
        s.append(&data.as_slice());
    };
    let mut unsigned = rlp::RlpStream::new_list(9);
    prefix(&mut unsigned);
    unsigned.append(&56u64);
    unsigned.append(&0u8);
    unsigned.append(&0u8);
    let digest = Keccak256::digest(unsigned.out());
    let (sig, recovery) = key
        .sign_prehash_recoverable(&digest)
        .map_err(|_| unavailable())?;
    let bytes = sig.to_bytes();
    let minimal = |v: &[u8]| {
        v.iter()
            .position(|b| *b != 0)
            .map(|p| v[p..].to_vec())
            .unwrap_or_default()
    };
    let mut signed = rlp::RlpStream::new_list(9);
    prefix(&mut signed);
    signed.append(&(56 * 2 + 35 + u64::from(recovery.to_byte())));
    signed.append(&minimal(&bytes[..32]).as_slice());
    signed.append(&minimal(&bytes[32..]).as_slice());
    let raw = signed.out();
    Ok((format!("0x{}", hex::encode(&raw)), bnb::hash(&raw)))
}
impl AppState {
    pub async fn sponsorship(&self) -> Value {
        let base = |status: &str, message: &str, address: Option<&str>, remaining: u64| json!({"status":status,"message":message,"chain_id":56,"sponsor_address":address,"daily_budget_bnb":wei(DAILY_WEI),"daily_remaining_bnb":wei(remaining),"per_transaction_max_bnb":wei(MAX_TX_WEI),"per_account_daily":PER_ACCOUNT_DAY,"per_wallet_daily":PER_WALLET_DAY,"global_daily":GLOBAL_DAY,"scope":"registration_only"});
        let Ok((_, address)) = signer(&self.config) else {
            return base(
                "not_configured",
                "Registration sponsorship is not configured.",
                None,
                0,
            );
        };
        if !self.config.sponsor_enabled {
            return base(
                "disabled",
                "Registration sponsorship is paused. No new gas transactions will be signed.",
                Some(&address),
                0,
            );
        }
        if self.bnb.require_deployment().await.is_err() {
            return base(
                "unavailable",
                "Registration contract verification is temporarily unavailable.",
                Some(&address),
                0,
            );
        }
        let Ok((count, used, pending)) = self.store.connect().and_then(|db| budget(&db)) else {
            return base(
                "unavailable",
                "Registration budget verification is temporarily unavailable.",
                Some(&address),
                0,
            );
        };
        let remaining = DAILY_WEI.saturating_sub(used);
        if count >= GLOBAL_DAY || remaining < MAX_TX_WEI {
            return base(
                "budget_exhausted",
                "Today's registration sponsorship budget is reached.",
                Some(&address),
                remaining,
            );
        }
        let balance = self
            .bnb
            .rpc("eth_getBalance", json!([address, "latest"]))
            .await
            .and_then(|v| n(&v));
        match balance {
            Ok(v)
                if v >= pending
                    .saturating_add(MAX_TX_WEI)
                    .saturating_add(RESERVE_WEI) =>
            {
                base(
                    "ready",
                    "Tab covers registration gas within its daily sponsorship limits.",
                    Some(&address),
                    remaining,
                )
            }
            Ok(_) => base(
                "unfunded",
                "The registration sponsor needs a BNB top-up.",
                Some(&address),
                remaining,
            ),
            Err(_) => base(
                "unavailable",
                "The registration sponsor balance could not be verified.",
                Some(&address),
                remaining,
            ),
        }
    }
    pub async fn sponsored_challenge(
        &self,
        owner: &str,
        id: &str,
        wallet: &str,
    ) -> Result<SponsoredChallenge> {
        let wallet = bnb::address(wallet)?;
        if wallet == bnb::ZERO {
            return Err(ApiError::bad("Use the wallet that will own this agent."));
        }
        let agent = self.store.agent(owner, id)?;
        if agent.registry_id.is_some() {
            return Err(ApiError::conflict("Agent is already registered."));
        }
        if !self.config.sponsor_enabled {
            return Err(ApiError::unavailable(
                "Registration sponsorship is paused. No new gas transactions will be signed.",
            ));
        }
        let (_, address) = signer(&self.config)?;
        self.bnb.require_deployment().await?;
        let nonce = n(&self
            .bnb
            .view("protocol", "registrationNonces", vec![bnb::addr(&wallet)?])
            .await?)?;
        let balance = n(&self
            .bnb
            .rpc("eth_getBalance", json!([address, "latest"]))
            .await?)?;
        let mut db = self.store.connect()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let fresh = Store::agent_in(&tx, owner, id)?;
        ensure_unsigned(&tx, id)?;
        if fresh.registry_id.is_some() || fresh.registration_tx.is_some() {
            return Err(ApiError::conflict(
                "Agent registration is already submitted.",
            ));
        }
        let inflight: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sponsor_jobs WHERE wallet=? AND status='pending')",
            [&wallet],
            |r| r.get(0),
        )?;
        if inflight {
            return Err(ApiError::conflict("This wallet has a registration pending. Confirm it before registering another agent."));
        }
        quota(&tx, owner, &wallet, MAX_TX_WEI, balance)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT payload FROM sponsor_challenges WHERE owner=? AND agent_id=?",
                params![owner, id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(raw) = existing {
            let terms: Terms = decode(raw)?;
            if terms.wallet == wallet
                && terms.deadline > Utc::now().timestamp() as u64
                && terms.nonce == nonce
                && terms.matches(self, &fresh)?
            {
                return terms.challenge();
            }
        }
        let terms = Terms {
            request_id: identifier(),
            agent_id: id.into(),
            wallet: wallet.clone(),
            registry: self.config.program.clone(),
            registry_id: self.bnb.agent_address(&wallet, id)?,
            name: fresh.plan.name.clone(),
            cap: units(fresh.plan.daily_cap)?.to_string(),
            policy: self.policy(&fresh),
            nonce,
            deadline: (Utc::now() + Duration::minutes(10)).timestamp() as u64,
        };
        tx.execute("INSERT INTO sponsor_challenges VALUES(?,?,?,?) ON CONFLICT(agent_id) DO UPDATE SET request_id=excluded.request_id,owner=excluded.owner,payload=excluded.payload",params![id,terms.request_id,owner,encode(&terms)?])?;
        tx.execute("DELETE FROM wallet_challenges WHERE agent_id=?", [id])?;
        tx.commit()?;
        terms.challenge()
    }
    pub async fn sponsor_registration(
        &self,
        owner: &str,
        id: &str,
        proof: RegistrationSignature,
    ) -> Result<SponsorshipResult> {
        let wallet = bnb::address(&proof.wallet)?;
        let agent = self.store.agent(owner, id)?;
        let existing = {
            let db = self.store.connect()?;
            let raw:Option<(String,String)>=db.query_row("SELECT payload,status FROM sponsor_jobs WHERE owner=? AND agent_id=? AND request_id=?",params![owner,id,proof.request_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            raw.map(|(payload, status)| {
                Ok::<Saved, ApiError>(Saved {
                    job: decode(payload)?,
                    status,
                    owner: owner.into(),
                })
            })
            .transpose()?
        };
        if let Some(saved) = existing {
            if saved.job.terms.wallet != wallet {
                return Err(ApiError::forbidden(
                    "This registration belongs to another wallet.",
                ));
            }
            return self.settle_sponsor(saved).await;
        }
        if agent.registry_id.is_some() {
            return Err(ApiError::conflict("Agent is already registered."));
        }
        if !self.config.sponsor_enabled {
            return Err(ApiError::unavailable(
                "Registration sponsorship is paused. No new gas transactions will be signed.",
            ));
        }
        let raw:Option<String>=self.store.connect()?.query_row("SELECT payload FROM sponsor_challenges WHERE owner=? AND agent_id=? AND request_id=?",params![owner,id,proof.request_id],|r|r.get(0)).optional()?;
        let terms: Terms = decode(raw.ok_or_else(|| {
            ApiError::conflict("The registration challenge changed. Review it again.")
        })?)?;
        if terms.wallet != wallet
            || !terms.matches(self, &agent)?
            || terms.deadline <= Utc::now().timestamp() as u64
        {
            return Err(ApiError::conflict(
                "The registration challenge expired or its policy changed. Review it again.",
            ));
        }
        let (key, sponsor) = signer(&self.config)?;
        self.bnb.require_deployment().await?;
        let code = self
            .bnb
            .rpc("eth_getCode", json!([wallet, "latest"]))
            .await?;
        if code == "0x" || code == "0x0" {
            if bnb::digest_signer(&terms.digest()?, &proof.signature)? != wallet {
                return Err(ApiError::forbidden(
                    "Registration signature belongs to another wallet.",
                ));
            }
        } else {
            let mut data = Keccak256::digest(b"isValidSignature(bytes32,bytes)")[..4].to_vec();
            let signature = hex::decode(proof.signature.strip_prefix("0x").unwrap_or(""))
                .map_err(|_| ApiError::bad("Invalid registration signature."))?;
            if signature.len() > 4096 {
                return Err(ApiError::bad("Registration signature is too large."));
            }
            data.extend(ethabi::encode(&[
                Token::FixedBytes(terms.digest()?),
                Token::Bytes(signature),
            ]));
            let valid = self
                .bnb
                .rpc(
                    "eth_call",
                    json!([{"to":wallet,"data":format!("0x{}",hex::encode(data)),"gas":"0x186a0"},"latest"]),
                )
                .await?;
            if !valid.as_str().is_some_and(|v| v.starts_with("0x1626ba7e")) {
                return Err(ApiError::forbidden(
                    "Contract wallet rejected the registration signature.",
                ));
            }
        }
        if n(&self
            .bnb
            .view("protocol", "registrationNonces", vec![bnb::addr(&wallet)?])
            .await?)?
            != terms.nonce
        {
            return Err(ApiError::conflict(
                "This wallet registered another agent. Review a fresh registration challenge.",
            ));
        }
        let ix = terms.instruction(self, &proof.signature)?;
        // eth_call and estimate execute the exact owner-authorized payload from the sponsor.
        let request = json!({"from":sponsor,"to":ix.to,"data":ix.data,"value":"0x0","gas":format!("0x{MAX_GAS:x}")});
        self.bnb.rpc("eth_call", json!([request, "latest"])).await?;
        let estimate = n(&self.bnb.rpc("eth_estimateGas", json!([request])).await?)?;
        let gas = estimate
            .checked_mul(125)
            .and_then(|v| v.checked_div(100))
            .and_then(|v| v.checked_add(1000))
            .ok_or_else(unavailable)?;
        let price = n(&self.bnb.rpc("eth_gasPrice", json!([])).await?)?
            .checked_mul(125)
            .and_then(|v| v.checked_div(100))
            .ok_or_else(unavailable)?;
        let reserve = gas.checked_mul(price).ok_or_else(unavailable)?;
        if gas > MAX_GAS || gas == 0 || price > MAX_GAS_PRICE || price == 0 || reserve > MAX_TX_WEI
        {
            return Err(ApiError::unavailable(
                "BNB gas is above the registration sponsorship limit. Try again later.",
            ));
        }
        let chain_nonce = n(&self
            .bnb
            .rpc("eth_getTransactionCount", json!([sponsor, "pending"]))
            .await?)?;
        let balance = n(&self
            .bnb
            .rpc("eth_getBalance", json!([sponsor, "latest"]))
            .await?)?;
        let job = (|| -> Result<Job> {
            let mut db = self.store.connect()?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            if let Some(saved) = current_job(&tx, owner, id)? {
                if saved.status == "pending" || saved.status == "confirmed" {
                    if saved.job.terms.request_id == proof.request_id {
                        return Ok(saved.job);
                    }
                    return Err(ApiError::conflict(
                        "Another registration is already submitted.",
                    ));
                }
            }
            let mut fresh = Store::agent_in(&tx, owner, id)?;
            let retained:Option<String>=tx.query_row("SELECT payload FROM sponsor_challenges WHERE owner=? AND agent_id=? AND request_id=?",params![owner,id,proof.request_id],|r|r.get(0)).optional()?;
            if fresh.registry_id.is_some()
                || fresh.registration_tx.is_some()
                || !terms.matches(self, &fresh)?
                || retained.as_deref() != Some(&encode(&terms)?)
                || terms.deadline <= Utc::now().timestamp() as u64
            {
                return Err(ApiError::conflict(
                    "Registration changed during preparation. Review it again.",
                ));
            }
            let inflight: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM sponsor_jobs WHERE wallet=? AND status='pending')",
                [&wallet],
                |r| r.get(0),
            )?;
            if inflight {
                return Err(ApiError::conflict(
                    "This wallet already has a registration pending.",
                ));
            }
            quota(&tx, owner, &wallet, reserve, balance)?;
            let previous: Option<u64> = tx.query_row(
                "SELECT max(nonce) FROM sponsor_jobs WHERE sponsor=?",
                [&sponsor],
                |r| r.get(0),
            )?;
            let nonce = chain_nonce.max(
                previous
                    .map(|v| v.checked_add(1).ok_or_else(unavailable))
                    .transpose()?
                    .unwrap_or(0),
            );
            if nonce > i64::MAX as u64 || terms.nonce > i64::MAX as u64 {
                return Err(unavailable());
            }
            let (raw_tx, tx_hash) = signed_transaction(&key, nonce, gas, price, &ix)?;
            let job = Job {
                terms,
                instruction: ix,
                sponsor: sponsor.clone(),
                nonce,
                gas,
                gas_price: price,
                raw_tx,
                tx_hash,
            };
            tx.execute("INSERT INTO sponsor_jobs(request_id,agent_id,owner,wallet,sponsor,nonce,owner_nonce,created_at,status,tx_hash,reserved_wei,payload) VALUES(?,?,?,?,?,?,?,?, 'pending',?,?,?)",params![job.terms.request_id,id,owner,wallet,sponsor,nonce,job.terms.nonce,now(),job.tx_hash,reserve,encode(&job)?])?;
            fresh.registration_tx = Some(job.tx_hash.clone());
            Store::save_agent_in(&tx, &fresh)?;
            tx.commit()?;
            Ok(job)
        })()?;
        self.settle_sponsor(saved_job(
            &self.store.connect()?,
            owner,
            &job.terms.request_id,
        )?)
        .await
    }
    pub async fn sponsor_status(&self, owner: &str, id: &str) -> Result<SponsorshipResult> {
        self.store.agent(owner, id)?;
        let saved = current_job(&self.store.connect()?, owner, id)?;
        if let Some(saved) = saved {
            return self.settle_sponsor(saved).await;
        }
        let request_id = self
            .store
            .connect()?
            .query_row(
                "SELECT request_id FROM sponsor_challenges WHERE owner=? AND agent_id=?",
                params![owner, id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(SponsorshipResult{status:"not_submitted".into(),request_id,retryable:true,tx_hash:None,agent:None,message:Some("No sponsored registration transaction has been submitted. Review and sign the current registration challenge.".into())})
    }
    async fn settle_sponsor(&self, saved: Saved) -> Result<SponsorshipResult> {
        let job = &saved.job;
        if saved.status != "pending" {
            return Ok(SponsorshipResult {
                status: saved.status.clone(),
                request_id: Some(job.terms.request_id.clone()),
                retryable: saved.status == "failed",
                tx_hash: Some(job.tx_hash.clone()),
                agent: if saved.status == "confirmed" {
                    Some(self.store.agent(&saved.owner, &job.terms.agent_id)?)
                } else {
                    None
                },
                message: if saved.status == "failed" {
                    Some("The sponsored transaction reverted. Its gas was accounted for; review a new registration.".into())
                } else {
                    None
                },
            });
        }
        let result = self.reconcile_sponsor(&saved).await;
        if result.is_ok_and(|done| done) {
            let latest = saved_job(&self.store.connect()?, &saved.owner, &job.terms.request_id)?;
            return Ok(SponsorshipResult {
                status: latest.status.clone(),
                request_id: Some(job.terms.request_id.clone()),
                retryable: latest.status == "failed",
                tx_hash: Some(job.tx_hash.clone()),
                agent: if latest.status == "confirmed" {
                    Some(self.store.agent(&saved.owner, &job.terms.agent_id)?)
                } else {
                    None
                },
                message: if latest.status == "failed" {
                    Some("The sponsored transaction reverted. Its gas was accounted for; review a new registration.".into())
                } else {
                    None
                },
            });
        }
        Ok(SponsorshipResult{status:"pending".into(),request_id:Some(job.terms.request_id.clone()),retryable:false,tx_hash:Some(job.tx_hash.clone()),agent:None,message:Some("Registration is saved and awaiting a verified BNB confirmation. Tab retries the same transaction; do not register again.".into())})
    }
    async fn reconcile_sponsor(&self, saved: &Saved) -> Result<bool> {
        let job = &saved.job;
        self.bnb.require_deployment().await?;
        let status = self.bnb.transaction_status(&job.tx_hash).await?;
        if status["status"] == "pending" {
            let existing = self
                .bnb
                .rpc("eth_getTransactionByHash", json!([job.tx_hash]))
                .await?;
            if existing.is_null() {
                // Only this immutable signed transaction may be retried, even after restart.
                // A timeout or nonce disagreement never releases its reservation.
                let bytes =
                    hex::decode(job.raw_tx.trim_start_matches("0x")).map_err(|_| unavailable())?;
                if bnb::hash(&bytes) != job.tx_hash
                    || job.instruction.to != self.config.program
                    || job.instruction.value != "0x0"
                {
                    return Err(unavailable());
                }
                let sent = self
                    .bnb
                    .rpc("eth_sendRawTransaction", json!([job.raw_tx]))
                    .await?;
                if sent.as_str() != Some(&job.tx_hash) {
                    return Err(unavailable());
                }
            }
            return Ok(false);
        }
        let tx = self
            .bnb
            .verify_envelope(&job.tx_hash, &job.sponsor, &job.instruction)
            .await?;
        let receipt = &status["receipt"];
        if tx["blockHash"] != receipt["blockHash"]
            || tx["blockNumber"] != receipt["blockNumber"]
            || receipt["from"].as_str().and_then(|v| bnb::address(v).ok())
                != Some(job.sponsor.clone())
            || receipt["to"].as_str().and_then(|v| bnb::address(v).ok())
                != Some(job.instruction.to.clone())
            || n(&tx["nonce"])? != job.nonce
            || n(&tx["gas"])? != job.gas
            || n(&tx["gasPrice"])? != job.gas_price
        {
            return Err(unavailable());
        }
        let used = n(&receipt["gasUsed"])?;
        let price = n(&receipt["effectiveGasPrice"])?;
        if used > job.gas || price != job.gas_price {
            return Err(unavailable());
        }
        let actual = used.checked_mul(price).ok_or_else(unavailable)?;
        let success = status["status"] == "confirmed";
        if success {
            // The canonical registration event proves the immutable original payload.
            // The owner may already have changed or paused its policy by the time we poll.
            let topic = bnb::hash(b"AgentRegistered(bytes32,address,string,uint256,bytes32)");
            let owner = format!("0x{:0>64}", job.terms.wallet.trim_start_matches("0x"));
            let data = format!(
                "0x{}",
                hex::encode(ethabi::encode(&[
                    Token::String(job.terms.name.clone()),
                    bnb::uint(job.terms.cap.parse().map_err(|_| unavailable())?),
                    bnb::bytes32(&job.terms.policy)?
                ]))
            );
            let count = receipt["logs"]
                .as_array()
                .ok_or_else(unavailable)?
                .iter()
                .filter(|log| {
                    log["address"]
                        .as_str()
                        .is_some_and(|v| v.eq_ignore_ascii_case(&job.instruction.to))
                        && log["topics"] == json!([topic, job.terms.registry_id, owner])
                        && log["data"] == data
                        && log["removed"] != true
                })
                .count();
            if count != 1 {
                return Err(unavailable());
            }
        }
        let mut db = self.store.connect()?;
        let dbtx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current: String = dbtx.query_row(
            "SELECT status FROM sponsor_jobs WHERE request_id=?",
            [&job.terms.request_id],
            |r| r.get(0),
        )?;
        if current != "pending" {
            return Ok(true);
        }
        let mut agent = Store::agent_in(&dbtx, &saved.owner, &job.terms.agent_id)?;
        if agent.registration_tx.as_deref() != Some(&job.tx_hash)
            || !job.terms.matches(self, &agent)?
        {
            return Err(ApiError::conflict(
                "The registered policy differs from the retained sponsorship request.",
            ));
        }
        let action = format!("registration:{}", agent.id);
        let claimed: Option<String> = dbtx
            .query_row(
                "SELECT action_id FROM chain_receipts WHERE tx_hash=?",
                [&job.tx_hash],
                |r| r.get(0),
            )
            .optional()?;
        if claimed.as_ref().is_some_and(|v| v != &action) {
            return Err(ApiError::conflict("Registration receipt is already used."));
        }
        if success {
            dbtx.execute(
                "INSERT OR IGNORE INTO chain_receipts VALUES(?,?,?)",
                params![job.tx_hash, action, now()],
            )?;
            agent.wallet = Some(job.terms.wallet.clone());
            agent.registry_id = Some(job.terms.registry_id.clone());
            agent.registry_address = job.terms.registry.clone();
            agent.status = "ready".into();
            agent.next_run = crate::runtime::next_run(&agent);
            dbtx.execute(
                "DELETE FROM wallet_challenges WHERE agent_id=?",
                [&agent.id],
            )?;
            dbtx.execute("INSERT INTO agent_events(agent_id,kind,status,at,message,provider,amount,currency,tx_hash) VALUES(?,'registered','confirmed',?,'agent registered on BNB Smart Chain; registration gas covered by Tab','bnb-rpc',NULL,NULL,?)",params![agent.id,now(),job.tx_hash])?;
        } else {
            agent.registration_tx = None;
        }
        Store::save_agent_in(&dbtx, &agent)?;
        dbtx.execute("UPDATE sponsor_jobs SET status=?,actual_wei=?,settled_at=? WHERE request_id=? AND status='pending'",params![if success{"confirmed"}else{"failed"},actual,now(),job.terms.request_id])?;
        dbtx.execute(
            "DELETE FROM sponsor_challenges WHERE agent_id=? AND request_id=?",
            params![agent.id, job.terms.request_id],
        )?;
        dbtx.commit()?;
        Ok(true)
    }
    pub async fn reconcile_sponsorships(&self) {
        let rows = (|| -> Result<Vec<(String, String)>> {
            let db = self.store.connect()?;
            let mut statement=db.prepare("SELECT owner,agent_id FROM sponsor_jobs WHERE status='pending' ORDER BY sponsor,nonce LIMIT 100")?;
            let rows = statement
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })();
        if let Ok(rows) = rows {
            for (owner, id) in rows {
                if self.sponsor_status(&owner, &id).await.is_err() {
                    tracing::warn!("Sponsored registration reconciliation unavailable");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain_tests::{fixture, Fixture};
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    fn test_key(n: u8) -> SigningKey {
        let mut bytes = [0u8; 32];
        bytes[31] = n;
        SigningKey::from_slice(&bytes).unwrap()
    }
    fn address(key: &SigningKey) -> String {
        let point = key.verifying_key().to_encoded_point(false);
        format!(
            "0x{}",
            hex::encode(&Keccak256::digest(&point.as_bytes()[1..])[12..])
        )
    }
    fn selector(method: &str) -> String {
        format!(
            "call:0x{}",
            hex::encode(&Keccak256::digest(method.as_bytes())[..4])
        )
    }
    fn abi(tokens: &[Token]) -> Value {
        json!(format!("0x{}", hex::encode(ethabi::encode(tokens))))
    }
    fn input() -> AgentInput {
        serde_json::from_value(json!({"name":"willow","purpose":"research public source documents","tools":["bnb-rpc"],"daily_cap":"1","max_call":"0.1","public_activity":false})).unwrap()
    }
    async fn setup() -> Fixture {
        let mut f = fixture().await;
        let mut config = (*f.state.config).clone();
        let key = test_key(2);
        config.sponsor_enabled = true;
        config.sponsor_key = Some(format!("0x{}", hex::encode(key.to_bytes())));
        config.sponsor_address = Some(address(&key));
        f.state = AppState::new(config).unwrap();
        let mut map = f.replies.lock().unwrap();
        map.insert(format!("code:{}", address(&test_key(1))), json!("0x"));
        map.insert(format!("code:{}", address(&test_key(3))), json!("0x"));
        map.insert(
            selector("registrationNonces(address)"),
            abi(&[bnb::uint(0)]),
        );
        map.insert(selector("registerWithSignature(bytes32,address,string,uint256,bytes32,uint256,uint256,bytes)"),json!("0x"));
        map.insert("eth_getBalance".into(), json!("0xde0b6b3a7640000"));
        map.insert("eth_getTransactionCount".into(), json!("0x0"));
        map.insert("eth_gasPrice".into(), json!("0x5f5e100"));
        map.insert("eth_getTransactionReceipt".into(), Value::Null);
        map.insert("eth_getTransactionByHash".into(), Value::Null);
        drop(map);
        f
    }
    async fn preparation(
        f: &Fixture,
        owner: &str,
        key: u8,
    ) -> (RuntimeAgent, RegistrationSignature) {
        let agent = f.state.create_agent(owner, input()).unwrap();
        let wallet = address(&test_key(key));
        let challenge = f
            .state
            .sponsored_challenge(owner, &agent.id, &wallet)
            .await
            .unwrap();
        let terms: Terms = decode(
            f.state
                .store
                .connect()
                .unwrap()
                .query_row(
                    "SELECT payload FROM sponsor_challenges WHERE agent_id=?",
                    [&agent.id],
                    |r| r.get(0),
                )
                .unwrap(),
        )
        .unwrap();
        let (sig, v) = test_key(key)
            .sign_prehash_recoverable(&terms.digest().unwrap())
            .unwrap();
        let signature = format!("0x{}{:02x}", hex::encode(sig.to_bytes()), 27 + v.to_byte());
        (
            agent,
            RegistrationSignature {
                wallet,
                signature,
                request_id: challenge.request_id,
            },
        )
    }
    fn job(f: &Fixture, owner: &str, id: &str) -> Job {
        current_job(&f.state.store.connect().unwrap(), owner, id)
            .unwrap()
            .unwrap()
            .job
    }
    fn finalize(f: &Fixture, job: &Job, success: bool, head: u64) {
        let block = format!("0x{}", "6".repeat(64));
        let owner = format!("0x{:0>64}", job.terms.wallet.trim_start_matches("0x"));
        let log = json!({"address":job.instruction.to,"topics":[bnb::hash(b"AgentRegistered(bytes32,address,string,uint256,bytes32)"),job.terms.registry_id,owner],"data":abi(&[Token::String(job.terms.name.clone()),bnb::uint(job.terms.cap.parse().unwrap()),bnb::bytes32(&job.terms.policy).unwrap()])});
        let receipt = json!({"transactionHash":job.tx_hash,"status":if success{"0x1"}else{"0x0"},"blockNumber":"0x64","blockHash":block,"from":job.sponsor,"to":job.instruction.to,"gasUsed":"0x186a0","effectiveGasPrice":format!("0x{:x}",job.gas_price),"logs":if success{vec![log]}else{vec![]}});
        let tx = json!({"hash":job.tx_hash,"chainId":"0x38","from":job.sponsor,"to":job.instruction.to,"input":job.instruction.data,"value":"0x0","nonce":format!("0x{:x}",job.nonce),"gas":format!("0x{:x}",job.gas),"gasPrice":format!("0x{:x}",job.gas_price),"blockNumber":"0x64","blockHash":block});
        let mut map = f.replies.lock().unwrap();
        map.insert("eth_getTransactionReceipt".into(), receipt);
        map.insert("eth_getTransactionByHash".into(), tx);
        map.insert("eth_blockNumber".into(), json!(format!("0x{head:x}")));
        map.insert(
            selector("getAgent(bytes32)"),
            abi(&[Token::Tuple(vec![
                bnb::addr(&job.terms.wallet).unwrap(),
                Token::String(job.terms.name.clone()),
                bnb::uint(job.terms.cap.parse().unwrap()),
                bnb::bytes32(&job.terms.policy).unwrap(),
                Token::Bool(false),
                bnb::uint(1),
                bnb::uint(0),
                bnb::uint(0),
            ])]),
        );
    }
    #[test]
    fn independent_viem_eip712_and_legacy_transaction_vectors() {
        let v: Value =
            serde_json::from_str(include_str!("../testdata/bnb-sponsor-vector.json")).unwrap();
        let m = &v["typed_data"]["message"];
        let terms = Terms {
            request_id: "fixture".into(),
            agent_id: "fixture".into(),
            wallet: m["owner"].as_str().unwrap().to_lowercase(),
            registry: v["typed_data"]["domain"]["verifyingContract"]
                .as_str()
                .unwrap()
                .into(),
            registry_id: m["id"].as_str().unwrap().into(),
            name: m["name"].as_str().unwrap().into(),
            cap: m["dailyCap"].as_str().unwrap().into(),
            policy: m["policyHash"].as_str().unwrap().into(),
            nonce: m["nonce"].as_str().unwrap().parse().unwrap(),
            deadline: m["deadline"].as_str().unwrap().parse().unwrap(),
        };
        assert_eq!(
            format!("0x{}", hex::encode(terms.digest().unwrap())),
            v["digest"]
        );
        assert_eq!(
            bnb::digest_signer(&terms.digest().unwrap(), v["signature"].as_str().unwrap()).unwrap(),
            terms.wallet
        );
        let tx = &v["transaction"];
        let ix = Instruction {
            to: tx["to"].as_str().unwrap().into(),
            data: tx["calldata"].as_str().unwrap().into(),
            value: "0x0".into(),
        };
        let (raw, hash) = signed_transaction(&test_key(2), 9, 300000, 100000000, &ix).unwrap();
        assert_eq!(raw, tx["raw"]);
        assert_eq!(hash, tx["hash"]);
        for field in [
            "wallet",
            "policy",
            "name",
            "nonce",
            "deadline",
            "registry",
            "registry_id",
            "cap",
        ] {
            let mut altered = terms.clone();
            match field {
                "wallet" => altered.wallet = address(&test_key(3)),
                "policy" => altered.policy = bnb::hash(b"another policy"),
                "name" => altered.name.push('x'),
                "nonce" => altered.nonce += 1,
                "deadline" => altered.deadline += 1,
                "registry" => altered.registry = address(&test_key(3)),
                "registry_id" => altered.registry_id = bnb::hash(b"another id"),
                _ => altered.cap = "2".into(),
            };
            assert_ne!(
                bnb::digest_signer(&altered.digest().unwrap(), v["signature"].as_str().unwrap())
                    .unwrap(),
                terms.wallet
            );
        }
    }
    #[tokio::test]
    async fn durable_timeout_restart_and_exact_success_are_idempotent() {
        let f = setup().await;
        let (agent, proof) = preparation(&f, "alice", 1).await;
        assert_eq!(
            f.state
                .sponsor_status("alice", &agent.id)
                .await
                .unwrap()
                .status,
            "not_submitted"
        );
        f.replies
            .lock()
            .unwrap()
            .insert("broadcast_error".into(), json!(true));
        let first = f
            .state
            .sponsor_registration("alice", &agent.id, proof.clone())
            .await
            .unwrap();
        assert_eq!(first.status, "pending");
        assert!(!first.retryable);
        let saved = job(&f, "alice", &agent.id);
        assert_eq!(first.tx_hash, Some(saved.tx_hash.clone()));
        assert!(f.state.update_agent("alice", &agent.id, input()).is_err());
        assert!(f
            .state
            .challenge("alice", &agent.id, &proof.wallet)
            .await
            .is_err());
        let restarted = AppState::new((*f.state.config).clone()).unwrap();
        let again = restarted.sponsor_status("alice", &agent.id).await.unwrap();
        assert_eq!(again.tx_hash, first.tx_hash);
        let sent = f.replies.lock().unwrap()["sent"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0], sent[1]);
        finalize(&f, &saved, true, 102);
        f.replies
            .lock()
            .unwrap()
            .insert(selector("getAgent(bytes32)"), json!("0xdead"));
        let done = restarted.sponsor_status("alice", &agent.id).await.unwrap();
        assert_eq!(done.status, "confirmed");
        assert_eq!(done.agent.unwrap().wallet, Some(proof.wallet.clone()));
        assert_eq!(
            f.state
                .sponsor_registration("alice", &agent.id, proof)
                .await
                .unwrap()
                .status,
            "confirmed"
        );
        assert!(restarted.sponsor_status("bob", &agent.id).await.is_err());
        let db = f.state.store.connect().unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM sponsor_jobs", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM agent_events WHERE kind='registered'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(budget(&db).unwrap().1, 100000 * saved.gas_price);
    }
    #[tokio::test]
    async fn owner_signature_policy_expiry_nonce_and_gas_gates_never_broadcast() {
        let f = setup().await;
        let (agent, proof) = preparation(&f, "alice", 1).await;
        let mut bad = proof.clone();
        bad.wallet = address(&test_key(3));
        assert!(f
            .state
            .sponsor_registration("alice", &agent.id, bad)
            .await
            .is_err());
        assert!(f
            .state
            .sponsor_registration("bob", &agent.id, proof.clone())
            .await
            .is_err());
        for (key, value) in [
            ("eth_getBalance", json!("0x0")),
            ("eth_estimateGas", json!("0x989680")),
            ("eth_gasPrice", json!("0x3b9aca01")),
            ("eth_chainId", json!("0x1")),
        ] {
            let old = f.replies.lock().unwrap().insert(key.into(), value);
            assert!(f
                .state
                .sponsor_registration("alice", &agent.id, proof.clone())
                .await
                .is_err());
            f.replies.lock().unwrap().insert(key.into(), old.unwrap());
        }
        f.replies.lock().unwrap().insert(
            selector("registrationNonces(address)"),
            abi(&[bnb::uint(1)]),
        );
        assert!(f
            .state
            .sponsor_registration("alice", &agent.id, proof.clone())
            .await
            .is_err());
        f.replies.lock().unwrap().insert(
            selector("registrationNonces(address)"),
            abi(&[bnb::uint(0)]),
        );
        let mut change = input();
        change.name = "changed".into();
        f.state.update_agent("alice", &agent.id, change).unwrap();
        assert!(f
            .state
            .sponsor_registration("alice", &agent.id, proof)
            .await
            .is_err());
        assert!(f.replies.lock().unwrap().get("sent").is_none());
        let (agent, proof) = preparation(&f, "alice", 1).await;
        let db = f.state.store.connect().unwrap();
        let mut terms: Terms = decode(
            db.query_row(
                "SELECT payload FROM sponsor_challenges WHERE agent_id=?",
                [&agent.id],
                |r| r.get(0),
            )
            .unwrap(),
        )
        .unwrap();
        terms.deadline = 1;
        db.execute(
            "UPDATE sponsor_challenges SET payload=? WHERE agent_id=?",
            params![encode(&terms).unwrap(), agent.id],
        )
        .unwrap();
        assert!(f
            .state
            .sponsor_registration("alice", &agent.id, proof)
            .await
            .is_err());
    }
    #[tokio::test]
    async fn operator_pause_blocks_new_signatures_but_preserves_exact_pending_recovery() {
        let f = setup().await;
        let (a, p) = preparation(&f, "alice", 1).await;
        let mut config = (*f.state.config).clone();
        config.sponsor_enabled = false;
        let paused = AppState::new(config).unwrap();
        assert_eq!(paused.sponsorship().await["status"], "disabled");
        assert!(paused
            .sponsored_challenge("alice", &a.id, &p.wallet)
            .await
            .is_err());
        assert!(paused
            .sponsor_registration("alice", &a.id, p.clone())
            .await
            .is_err());
        assert!(f.replies.lock().unwrap().get("sent").is_none());
        let submitted = f
            .state
            .sponsor_registration("alice", &a.id, p)
            .await
            .unwrap();
        let recovered = paused.sponsor_status("alice", &a.id).await.unwrap();
        assert_eq!(submitted.tx_hash, recovered.tx_hash);
        assert!(!recovered.retryable);
    }
    #[tokio::test]
    async fn contract_wallet_signature_and_registration_simulation_have_bounded_rpc_gas() {
        let f = setup().await;
        let (a, p) = preparation(&f, "alice", 1).await;
        let key = selector("isValidSignature(bytes32,bytes)");
        f.replies
            .lock()
            .unwrap()
            .insert(format!("code:{}", p.wallet), json!("0x6000"));
        f.replies
            .lock()
            .unwrap()
            .insert(key.clone(), json!(format!("0x1626ba7e{}", "00".repeat(28))));
        assert_eq!(
            f.state
                .sponsor_registration("alice", &a.id, p)
                .await
                .unwrap()
                .status,
            "pending"
        );
        let replies = f.replies.lock().unwrap();
        assert_eq!(
            replies[&format!("request:{key}")]["params"][0]["gas"],
            "0x186a0"
        );
        let register = selector(
            "registerWithSignature(bytes32,address,string,uint256,bytes32,uint256,uint256,bytes)",
        );
        assert_eq!(
            replies[&format!("request:{register}")]["params"][0]["gas"],
            "0x7a120"
        );
        assert_eq!(
            replies["request:eth_estimateGas"]["params"][0]["gas"],
            "0x7a120"
        );
    }
    #[tokio::test]
    async fn reverted_receipts_need_finality_and_canonical_exact_envelope_before_release() {
        let f = setup().await;
        let (agent, proof) = preparation(&f, "alice", 1).await;
        f.state
            .sponsor_registration("alice", &agent.id, proof)
            .await
            .unwrap();
        let saved = job(&f, "alice", &agent.id);
        finalize(&f, &saved, false, 100);
        assert_eq!(
            f.state
                .sponsor_status("alice", &agent.id)
                .await
                .unwrap()
                .status,
            "pending"
        );
        f.replies
            .lock()
            .unwrap()
            .insert("eth_blockNumber".into(), json!("0x66"));
        f.replies
            .lock()
            .unwrap()
            .get_mut("eth_getTransactionReceipt")
            .unwrap()["blockHash"] = json!(bnb::hash(b"wrong block"));
        assert_eq!(
            f.state
                .sponsor_status("alice", &agent.id)
                .await
                .unwrap()
                .status,
            "pending"
        );
        finalize(&f, &saved, false, 102);
        f.replies
            .lock()
            .unwrap()
            .get_mut("eth_getTransactionByHash")
            .unwrap()["input"] = json!("0x1234");
        assert_eq!(
            f.state
                .sponsor_status("alice", &agent.id)
                .await
                .unwrap()
                .status,
            "pending"
        );
        finalize(&f, &saved, false, 102);
        let failed = f.state.sponsor_status("alice", &agent.id).await.unwrap();
        assert_eq!(failed.status, "failed");
        assert!(failed.retryable);
        assert!(f
            .state
            .store
            .agent("alice", &agent.id)
            .unwrap()
            .registration_tx
            .is_none());
        assert!(budget(&f.state.store.connect().unwrap()).unwrap().1 > 0);
        assert!(f
            .state
            .sponsored_challenge("alice", &agent.id, &saved.terms.wallet)
            .await
            .is_ok());
    }
    #[tokio::test]
    async fn success_requires_exact_registration_event_and_keeps_budget_on_bad_proofs() {
        let f = setup().await;
        let (a, p) = preparation(&f, "alice", 1).await;
        let mut wrong = p.clone();
        wrong.signature = format!("0x{}", "00".repeat(65));
        assert!(f
            .state
            .sponsor_registration("alice", &a.id, wrong)
            .await
            .is_err());
        f.state
            .sponsor_registration("alice", &a.id, p)
            .await
            .unwrap();
        let saved = job(&f, "alice", &a.id);
        for field in [
            "address", "data", "owner", "id", "missing", "price", "nonce",
        ] {
            finalize(&f, &saved, true, 102);
            let mut replies = f.replies.lock().unwrap();
            match field {
                "address" => {
                    replies.get_mut("eth_getTransactionReceipt").unwrap()["logs"][0]["address"] =
                        json!(bnb::ZERO)
                }
                "data" => {
                    replies.get_mut("eth_getTransactionReceipt").unwrap()["logs"][0]["data"] =
                        json!("0x")
                }
                "owner" => {
                    replies.get_mut("eth_getTransactionReceipt").unwrap()["logs"][0]["topics"][2] =
                        json!(bnb::ZERO_HASH)
                }
                "id" => {
                    replies.get_mut("eth_getTransactionReceipt").unwrap()["logs"][0]["topics"][1] =
                        json!(bnb::ZERO_HASH)
                }
                "missing" => {
                    replies.get_mut("eth_getTransactionReceipt").unwrap()["logs"] = json!([])
                }
                "price" => {
                    replies.get_mut("eth_getTransactionReceipt").unwrap()["effectiveGasPrice"] =
                        json!("0x0")
                }
                _ => replies.get_mut("eth_getTransactionByHash").unwrap()["nonce"] = json!("0x99"),
            };
            drop(replies);
            assert_eq!(
                f.state.sponsor_status("alice", &a.id).await.unwrap().status,
                "pending"
            );
            assert!(budget(&f.state.store.connect().unwrap()).unwrap().2 > 0);
        }
        finalize(&f, &saved, true, 102);
        assert_eq!(
            f.state.sponsor_status("alice", &a.id).await.unwrap().status,
            "confirmed"
        );
    }
    #[tokio::test]
    async fn competing_requests_preserve_one_owner_nonce_and_distinct_relayer_nonces() {
        let f = setup().await;
        let (a, p) = preparation(&f, "alice", 1).await;
        let (b, q) = preparation(&f, "alice", 1).await;
        let (r, s) = tokio::join!(
            f.state.sponsor_registration("alice", &a.id, p.clone()),
            f.state.sponsor_registration("alice", &b.id, q)
        );
        assert!(r.is_ok() ^ s.is_ok());
        assert_eq!(
            f.state
                .store
                .connect()
                .unwrap()
                .query_row("SELECT count(*) FROM sponsor_jobs", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            1
        );
        let (c, t) = preparation(&f, "bob", 3).await;
        f.state.sponsor_registration("bob", &c.id, t).await.unwrap();
        let db = f.state.store.connect().unwrap();
        assert_eq!(
            db.query_row("SELECT count(DISTINCT nonce) FROM sponsor_jobs", [], |r| {
                r.get::<_, u64>(0)
            })
            .unwrap(),
            2
        );
    }
    #[tokio::test]
    async fn simultaneous_same_request_is_idempotent_across_service_instances() {
        let f = setup().await;
        let (a, p) = preparation(&f, "alice", 1).await;
        let other = AppState::new((*f.state.config).clone()).unwrap();
        let (r, s) = tokio::join!(
            f.state.sponsor_registration("alice", &a.id, p.clone()),
            other.sponsor_registration("alice", &a.id, p)
        );
        assert_eq!(r.unwrap().tx_hash, s.unwrap().tx_hash);
        assert_eq!(
            f.state
                .store
                .connect()
                .unwrap()
                .query_row("SELECT count(*) FROM sponsor_jobs", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            1
        );
    }
    #[tokio::test]
    async fn pending_budget_survives_midnight_and_account_wallet_limits_are_atomic() {
        let f = setup().await;
        let (a, p) = preparation(&f, "alice", 1).await;
        f.state
            .sponsor_registration("alice", &a.id, p)
            .await
            .unwrap();
        let db = f.state.store.connect().unwrap();
        db.execute(
            "UPDATE sponsor_jobs SET created_at='2000-01-01',reserved_wei=?",
            [DAILY_WEI],
        )
        .unwrap();
        assert_eq!(budget(&db).unwrap().1, DAILY_WEI);
        assert!(quota(&db, "bob", &address(&test_key(3)), 1, u64::MAX).is_err());
        db.execute(
            "UPDATE sponsor_jobs SET status='failed',actual_wei=?,settled_at=?",
            params![DAILY_WEI, now()],
        )
        .unwrap();
        assert!(quota(&db, "bob", &address(&test_key(3)), 1, u64::MAX).is_err());
        db.execute(
            "UPDATE sponsor_jobs SET actual_wei=1,created_at=?,settled_at=?",
            params![now(), now()],
        )
        .unwrap();
        for index in 1..3 {
            db.execute("INSERT INTO sponsor_jobs SELECT request_id||?,agent_id||?,owner,wallet,sponsor,nonce+?,owner_nonce,created_at,status,tx_hash||?,reserved_wei,actual_wei,settled_at,payload FROM sponsor_jobs LIMIT 1",params![index,index,index,index]).unwrap();
        }
        assert!(quota(&db, "alice", &address(&test_key(3)), 0, u64::MAX).is_err());
        assert!(quota(&db, "another", &address(&test_key(1)), 0, u64::MAX).is_err());
        assert!(quota(&db, "bob", &address(&test_key(3)), MAX_TX_WEI, RESERVE_WEI).is_err());
    }
    #[tokio::test]
    async fn sponsor_routes_require_privy_and_public_state_never_exposes_signer_material() {
        let f = setup().await;
        let agent = f.state.create_agent("alice", input()).unwrap();
        let mut cfg = (*f.state.config).clone();
        cfg.app_id = Some("test-app".into());
        let state = AppState::new(cfg).unwrap();
        for suffix in ["sponsor", "sponsor/status", "challenge"] {
            let body = if suffix == "sponsor" {
                json!({"wallet":address(&test_key(1)),"signature":"0x00","request_id":"unknown"})
            } else {
                json!({"wallet":address(&test_key(1))})
            };
            let response = crate::api::router(state.clone())
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/account/runtime/{}/{suffix}", agent.id))
                        .header("content-type", "application/json")
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
        let public = state.sponsorship().await.to_string();
        assert!(public.contains("ready"));
        assert!(!public.contains(state.config.sponsor_key.as_ref().unwrap()));
        f.replies
            .lock()
            .unwrap()
            .insert("eth_getBalance".into(), json!("0x0"));
        assert_eq!(state.sponsorship().await["status"], "unfunded");
    }
}
