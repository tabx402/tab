//! Wallet-authorized Permit2 payments with persistent quote reservations.
use crate::{
    bnb,
    db::{decode, encode, Store},
    error::{ApiError, Result},
    models::{identifier, money, now, units},
    x402::{self, Authorization, Merchant, Offer},
    AppState,
};
use chrono::Utc;
use ethabi::{ethereum_types::U256, Token};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Clone, Serialize, Deserialize)]
struct Quote {
    id: String,
    agent_id: String,
    merchant: Merchant,
    offer: Offer,
    authorization: Authorization,
    day: i64,
    expires: i64,
    #[serde(default)]
    task_hash: String,
}
impl AppState {
    async fn x402_chain_ready(&self) -> Result<()> {
        self.bnb.require_deployment().await?;
        for (address, expected) in [
            (
                x402::PERMIT2,
                "0x48774d936722dd7002887f307f58bcddb3eeabad39149e7dcb5c08e4ebe3310f",
            ),
            (
                x402::EXACT_PROXY,
                "0xce6429c0bb49284660683287c0a8fe548a88379072327d026626909d202048b9",
            ),
        ] {
            let value = self
                .bnb
                .rpc("eth_getCode", json!([address, "latest"]))
                .await?;
            let bytes = hex::decode(value.as_str().unwrap_or("").trim_start_matches("0x"))
                .map_err(|_| ApiError::unavailable("Invalid payment bytecode."))?;
            if bnb::hash(&bytes) != expected {
                return Err(ApiError::unavailable(
                    "The configured Permit2 contracts could not be verified.",
                ));
            }
        }
        Ok(())
    }
    async fn x402_agent_wallet(
        &self,
        owner: &str,
        id: &str,
    ) -> Result<(crate::models::RuntimeAgent, String)> {
        let agent = self.store.agent(owner, id)?;
        if agent.status != "ready" || !agent.plan.tools.iter().any(|t| t == "x402") {
            return Err(ApiError::conflict(
                "Enable x402 on a registered, running agent first.",
            ));
        }
        let wallet = agent
            .wallet
            .clone()
            .ok_or_else(|| ApiError::conflict("Register the agent first."))?;
        let key = self.bnb.agent_address(&wallet, id)?;
        if agent.registry_address != self.config.program
            || agent.registry_id.as_deref() != Some(key.as_str())
        {
            return Err(ApiError::conflict(
                "Agent registration belongs to another deployment.",
            ));
        }
        let state = self.bnb.agent(&key).await?;
        if state["owner"] != wallet || state["paused"] != false {
            return Err(ApiError::forbidden(
                "Agent owner or paused state has changed.",
            ));
        }
        Ok((agent, wallet))
    }
    pub async fn x402_quote(&self, owner: &str, id: &str, merchant_id: &str) -> Result<Value> {
        self.x402_chain_ready().await?;
        let (agent, wallet) = self.x402_agent_wallet(owner, id).await?;
        let pending: bool = self.store.connect()?.query_row("SELECT EXISTS(SELECT 1 FROM x402_quotes WHERE agent_id=? AND status IN ('submitted','uncertain'))",[id],|r|r.get(0))?;
        if pending {
            return Err(ApiError::conflict(
                "Reconcile the previous payment before creating another paid request.",
            ));
        }
        let merchant = self
            .x402_merchants()?
            .into_iter()
            .find(|m| m.id == merchant_id)
            .ok_or_else(|| ApiError::missing("Allowlisted x402 merchant not found."))?;
        if !x402::sponsor_supported(&merchant).await? {
            return Err(ApiError::unavailable(
                "This facilitator has not advertised USDT Permit2 support.",
            ));
        }
        let chain = self
            .bnb
            .agent(agent.registry_id.as_deref().expect("verified agent"))
            .await?;
        let timestamp = Utc::now().timestamp();
        let day = timestamp / 86400;
        let chain_spent = if bnb::number(&chain["spendDay"])? == day as u128 {
            bnb::number(&chain["dailySpent"])?
        } else {
            0
        };
        let cap = units(agent.plan.daily_cap)?;
        let offer = x402::fetch_offer(
            &merchant,
            x402::NETWORK,
            &self.config.usdt,
            x402::Limits {
                per_call: units(agent.plan.max_call)?,
                daily_remaining: cap.saturating_sub(chain_spent),
                total_remaining: cap.saturating_sub(chain_spent),
            },
        )
        .await?;
        let (balance, decimals) = self.bnb.token_balance(&wallet, x402::USDT).await?;
        if decimals != 18 || balance < offer.amount_units {
            return Err(ApiError::conflict(
                "The connected wallet needs enough USDT for this request.",
            ));
        }
        let allowance = self
            .bnb
            .erc20(
                x402::USDT,
                "allowance(address,address)",
                vec![bnb::addr(&wallet)?, bnb::addr(x402::PERMIT2)?],
            )
            .await?;
        if allowance < U256::from(offer.amount_units) {
            let values = if allowance.is_zero() {
                vec![offer.amount_units]
            } else {
                vec![0, offer.amount_units]
            };
            let transactions:Vec<_>=values.into_iter().map(|n|{let selector=&bnb::hash(b"approve(address,uint256)")[2..10];let args=ethabi::encode(&[bnb::addr(x402::PERMIT2).expect("canonical address"),bnb::uint(n)]);json!({"to":x402::USDT,"data":format!("0x{}{}",selector,hex::encode(args)),"value":"0x0","chainId":"0x38"})}).collect();
            return Ok(
                json!({"provider":merchant.id,"offer":offer,"status":"approval_required","sender":wallet,"chain_id":56,"transactions":transactions,"currency":"USDT","message":"Approve this USDT amount for Permit2, then request a fresh quote."}),
            );
        }
        let authorization = Authorization::new(&offer, &wallet, timestamp as u64)?;
        let quote = Quote {
            id: identifier(),
            agent_id: id.into(),
            merchant,
            expires: authorization
                .deadline
                .parse()
                .map_err(|_| ApiError::internal())?,
            offer,
            authorization,
            day,
            task_hash: self.policy(&agent),
        };
        self.reserve_x402_quote(owner, &quote, cap, chain_spent, timestamp)?;
        Ok(
            json!({"quote_id":quote.id,"provider":quote.merchant.id,"offer":quote.offer,"typed_data":quote.authorization.typed_data(),"sender":wallet,"chain_id":56,"status":"requires_wallet_authorization","currency":"USDT","expires_at":quote.expires,"settlement_enabled":true}),
        )
    }
    fn reserve_x402_quote(
        &self,
        owner: &str,
        quote: &Quote,
        cap: u128,
        chain_spent: u128,
        timestamp: i64,
    ) -> Result<()> {
        let id = &quote.agent_id;
        let day = quote.day;
        let mut db = self.store.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let unsettled:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM x402_quotes WHERE agent_id=? AND status IN ('submitted','uncertain'))",[id],|r|r.get(0))?;
        if unsettled {
            return Err(ApiError::conflict(
                "Reconcile the submitted payment before reserving another request.",
            ));
        }
        let amounts: Vec<String> = {
            let mut stmt=tx.prepare("SELECT amount FROM x402_quotes WHERE agent_id=? AND day=? AND status!='cancelled' AND (status!='prepared' OR expires>?)")?;
            let rows = stmt
                .query_map(params![id, day, timestamp], |r| r.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        let reserved = amounts.iter().try_fold(0u128, |sum, n| {
            sum.checked_add(n.parse::<u128>().map_err(|_| ApiError::internal())?)
                .ok_or_else(ApiError::internal)
        })?;
        if chain_spent
            .saturating_add(reserved)
            .saturating_add(quote.offer.amount_units)
            > cap
        {
            return Err(ApiError::conflict(
                "Other payment reservations have used this agent's daily limit.",
            ));
        }
        tx.execute("INSERT INTO x402_quotes(id,owner,agent_id,payload,amount,day,expires,status) VALUES(?,?,?,?,?,?,?,'prepared')",params![quote.id,owner,id,encode(&quote)?,quote.offer.amount_units.to_string(),day,quote.expires])?;
        tx.commit()?;
        Ok(())
    }
    fn claim_x402_quote(
        &self,
        owner: &str,
        quote: &Quote,
        signature: &str,
        time: i64,
        cap: u128,
        chain_spent: u128,
    ) -> Result<()> {
        let mut db = self.store.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !quote.task_hash.is_empty()
            && self.policy(&Store::agent_in(&tx, owner, &quote.agent_id)?) != quote.task_hash
        {
            return Err(ApiError::conflict(
                "Agent task or policy changed; request a fresh price.",
            ));
        }
        let pending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM x402_quotes WHERE agent_id=? AND status IN ('submitted','uncertain'))",[&quote.agent_id],|r|r.get(0))?;
        if pending {
            return Err(ApiError::conflict(
                "Reconcile the submitted payment before sending another request.",
            ));
        }
        let amounts: Vec<String> = {
            let mut stmt=tx.prepare("SELECT amount FROM x402_quotes WHERE agent_id=? AND day=? AND status!='cancelled' AND (status!='prepared' OR expires>?)")?;
            let rows = stmt
                .query_map(params![quote.agent_id, quote.day, time], |r| r.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        let reserved = amounts.iter().try_fold(0u128, |sum, n| {
            sum.checked_add(n.parse::<u128>().map_err(|_| ApiError::internal())?)
                .ok_or_else(ApiError::internal)
        })?;
        if chain_spent.saturating_add(reserved) > cap {
            return Err(ApiError::conflict(
                "The current daily limit or other spending no longer permits this payment.",
            ));
        }
        if tx.execute("UPDATE x402_quotes SET status='submitted',signature=? WHERE id=? AND owner=? AND status='prepared' AND expires>?",params![signature,quote.id,owner,time])?!=1{return Err(ApiError::conflict("Payment quote was submitted or expired."));}
        tx.commit()?;
        Ok(())
    }
    pub fn x402_history(&self, owner: &str, id: &str) -> Result<Value> {
        self.store.agent(owner, id)?;
        let db = self.store.connect()?;
        let mut statement=db.prepare("SELECT payload,status,tx_hash FROM x402_quotes WHERE owner=? AND agent_id=? ORDER BY rowid DESC LIMIT 100")?;
        let rows = statement
            .query_map(params![owner, id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let values=rows.into_iter().map(|(payload,status,tx_hash)| { let q:Quote=decode(payload)?; let delivery=self.paid_delivery_history(owner,id,&q.id)?; Ok(json!({"quote_id":q.id,"provider":q.merchant.id,"status":if status=="prepared" && q.expires<=Utc::now().timestamp(){"expired"}else{status.as_str()},"expires_at":q.expires,"amount":money(q.offer.amount_units).to_string(),"currency":"USDT","tx_hash":tx_hash,"delivery":delivery})) }).collect::<Result<Vec<Value>>>()?;
        Ok(json!(values))
    }
    fn stored_x402(
        &self,
        owner: &str,
        id: &str,
        quote_id: &str,
    ) -> Result<(Quote, String, Option<String>, Option<String>)> {
        let row:Option<(String,String,Option<String>,Option<String>)>=self.store.connect()?.query_row("SELECT payload,status,signature,tx_hash FROM x402_quotes WHERE id=? AND owner=? AND agent_id=?",params![quote_id,owner,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let (payload, status, sig, hash) =
            row.ok_or_else(|| ApiError::missing("Payment quote not found."))?;
        Ok((decode(payload)?, status, sig, hash))
    }
    pub async fn x402_execute(
        &self,
        owner: &str,
        id: &str,
        quote_id: &str,
        signature: &str,
    ) -> Result<Value> {
        self.x402_chain_ready().await?;
        let (agent, wallet) = self.x402_agent_wallet(owner, id).await?;
        let (quote, status, _, _) = self.stored_x402(owner, id, quote_id)?;
        if !quote.task_hash.is_empty() && quote.task_hash != self.policy(&agent) {
            return Err(ApiError::conflict(
                "Agent task or policy changed; request a fresh price.",
            ));
        }
        if status != "prepared" {
            return Err(ApiError::conflict(
                "This quote was already submitted. Reconcile its receipt before another payment.",
            ));
        }
        if quote.offer.amount_units > units(agent.plan.max_call)?
            || quote.offer.amount_units > units(agent.plan.daily_cap)?
        {
            return Err(ApiError::conflict(
                "Agent spending limits changed; request a new quote.",
            ));
        }
        let current = self
            .x402_merchants()?
            .into_iter()
            .find(|m| m.id == quote.merchant.id)
            .ok_or_else(|| ApiError::forbidden("This merchant is no longer approved."))?;
        if encode(&current)? != encode(&quote.merchant)? {
            return Err(ApiError::conflict(
                "Merchant settings changed; request a new quote.",
            ));
        }
        let time = Utc::now().timestamp();
        if quote.day != time / 86400 {
            return Err(ApiError::conflict(
                "The payment day changed; request a fresh quote.",
            ));
        }
        quote
            .authorization
            .validate(&quote.offer, &wallet, time as u64)?;
        let header =
            x402::payment_signature(&quote.offer, &quote.authorization, signature, time as u64)?;
        let calldata = quote.authorization.settlement_calldata(signature)?;
        // Simulation proves nonce is unused, allowance and balance suffice, and the
        // signed exact recipient/amount is executable by the pinned proxy.
        self.bnb.rpc("eth_call",json!([{"from":if current.fee_payer.is_empty(){&wallet}else{&current.fee_payer},"to":x402::EXACT_PROXY,"data":calldata},"latest"])).await?;
        let chain = self
            .bnb
            .agent(agent.registry_id.as_deref().expect("verified agent"))
            .await?;
        if chain["paused"] != false {
            return Err(ApiError::conflict("The agent is paused onchain."));
        }
        let cap = units(agent.plan.daily_cap)?.min(bnb::number(&chain["dailyCap"])?);
        let chain_spent = if bnb::number(&chain["spendDay"])? == quote.day as u128 {
            bnb::number(&chain["dailySpent"])?
        } else {
            0
        };
        let claim_time = Utc::now().timestamp();
        if claim_time / 86400 != quote.day {
            return Err(ApiError::conflict(
                "The payment day changed; request a fresh quote.",
            ));
        }
        quote
            .authorization
            .validate(&quote.offer, &wallet, claim_time as u64)?;
        self.claim_x402_quote(owner, &quote, signature, claim_time, cap, chain_spent)?;
        let result = x402::retry_paid(&current, &header).await;
        let (body, receipt, service_ok) = match result {
            Ok(v) => v,
            Err(e) => {
                self.store.connect()?.execute(
                    "UPDATE x402_quotes SET status='uncertain' WHERE id=?",
                    [quote_id],
                )?;
                return Err(e);
            }
        };
        self.capture_paid_delivery(owner, id, quote_id, &receipt.transaction, &body, service_ok)?;
        if !receipt.payer.eq_ignore_ascii_case(&wallet) {
            return Err(ApiError::bad(
                "Merchant receipt belongs to a different payer. Reconcile onchain before retrying.",
            ));
        }
        let verified = self
            .x402_reconcile(owner, id, quote_id, &receipt.transaction)
            .await?;
        let delivery = self.paid_delivery_history(owner, id, quote_id)?;
        Ok(
            json!({"payment":verified,"result":String::from_utf8_lossy(&body),"service_status":delivery["status"],"delivery":delivery}),
        )
    }
    pub async fn x402_release_expired(
        &self,
        owner: &str,
        id: &str,
        quote_id: &str,
    ) -> Result<Value> {
        self.x402_chain_ready().await?;
        let (quote, status, _, _) = self.stored_x402(owner, id, quote_id)?;
        if status == "confirmed" {
            return Err(ApiError::conflict(
                "A confirmed payment cannot be released.",
            ));
        }
        let head = bnb::number(&self.bnb.rpc("eth_blockNumber", json!([])).await?)?;
        let block = format!(
            "0x{:x}",
            head.saturating_sub(u128::from(self.config.confirmations))
        );
        let settled = self
            .bnb
            .rpc("eth_getBlockByNumber", json!([block, false]))
            .await?;
        if bnb::number(&settled["timestamp"])? <= quote.expires as u128 {
            return Err(ApiError::conflict(
                "Wait for the authorization expiry to be confirmed on BNB.",
            ));
        }
        let nonce =
            U256::from_dec_str(&quote.authorization.nonce).map_err(|_| ApiError::internal())?;
        let selector = &bnb::hash(b"nonceBitmap(address,uint256)")[2..10];
        let data = format!(
            "0x{}{}",
            selector,
            hex::encode(ethabi::encode(&[
                bnb::addr(&quote.authorization.from)?,
                Token::Uint(nonce >> 8)
            ]))
        );
        let value = self
            .bnb
            .rpc("eth_call", json!([{"to":x402::PERMIT2,"data":data},block]))
            .await?;
        let bitmap =
            U256::from_str_radix(value.as_str().unwrap_or("").trim_start_matches("0x"), 16)
                .map_err(|_| ApiError::unavailable("Could not verify the Permit2 nonce."))?;
        if bitmap.bit((nonce.low_u32() & 255) as usize) {
            return Err(ApiError::conflict("This authorization was used. Reconcile its transaction hash before another payment."));
        }
        self.store.connect()?.execute("UPDATE x402_quotes SET status='cancelled' WHERE id=? AND owner=? AND status!='confirmed'",params![quote_id,owner])?;
        Ok(
            json!({"quote_id":quote_id,"status":"cancelled","message":"The expired authorization was unused; its reserved budget is released."}),
        )
    }
    pub async fn x402_reconcile(
        &self,
        owner: &str,
        id: &str,
        quote_id: &str,
        tx_hash: &str,
    ) -> Result<Value> {
        self.x402_chain_ready().await?;
        let (quote, status, signature, stored_hash) = self.stored_x402(owner, id, quote_id)?;
        if status == "prepared" {
            return Err(ApiError::conflict(
                "This quote has no submitted wallet authorization.",
            ));
        }
        if status == "confirmed"
            && stored_hash
                .as_ref()
                .is_some_and(|h| !h.eq_ignore_ascii_case(tx_hash))
        {
            return Err(ApiError::conflict(
                "This quote already has a different transaction hash.",
            ));
        }
        let signature = signature
            .ok_or_else(|| ApiError::conflict("Missing submitted wallet authorization."))?;
        let state = self.bnb.transaction_status(tx_hash).await?;
        if state["status"] != "confirmed" {
            return Ok(json!({"status":state["status"],"quote_id":quote_id,"tx_hash":tx_hash}));
        }
        let tx = self
            .bnb
            .rpc("eth_getTransactionByHash", json!([tx_hash]))
            .await?;
        if tx["blockHash"] != state["receipt"]["blockHash"]
            || tx["blockNumber"] != state["receipt"]["blockNumber"]
        {
            return Err(ApiError::bad(
                "Receipt and transaction are not in the same canonical block.",
            ));
        }
        x402::verify_transfer(
            &state["receipt"],
            &tx,
            tx_hash,
            &quote.authorization,
            &signature,
            &quote.merchant,
        )?;
        let mut db = self.store.connect()?;
        let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let used: Option<String> = transaction
            .query_row(
                "SELECT action_id FROM chain_receipts WHERE tx_hash=?",
                [tx_hash.to_lowercase()],
                |r| r.get(0),
            )
            .optional()?;
        if used.as_ref().is_some_and(|s| s != quote_id) {
            return Err(ApiError::conflict(
                "This payment receipt already confirms another action.",
            ));
        }
        let changed=transaction.execute("UPDATE x402_quotes SET status='confirmed',tx_hash=? WHERE id=? AND status!='confirmed'",params![tx_hash.to_lowercase(),quote_id])?;
        if changed == 1 {
            transaction.execute(
                "INSERT OR IGNORE INTO chain_receipts VALUES(?,?,?)",
                params![tx_hash.to_lowercase(), quote_id, now()],
            )?;
            transaction.execute("INSERT INTO agent_events(agent_id,kind,status,at,message,provider,amount,currency,tx_hash) VALUES(?,'payment','confirmed',?,'verified USDT payment',?,?,'USDT',?)",params![id,now(),quote.merchant.id,money(quote.offer.amount_units).to_string(),tx_hash.to_lowercase()])?;
        }
        transaction.commit()?;
        Ok(
            json!({"status":"confirmed","quote_id":quote_id,"tx_hash":tx_hash,"amount":money(quote.offer.amount_units).to_string(),"currency":"USDT"}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        let config = crate::config::Config {
            sponsor_enabled: false,
            sponsor_key: None,
            sponsor_address: None,
            network: "mainnet".into(),
            chain_id: 56,
            confirmations: 3,
            rpc: "http://127.0.0.1:1".into(),
            logs_rpc: None,
            usdt: bnb::USDT.into(),
            program: String::new(),
            official_tab: None,
            app_id: None,
            database: r.join("test.sqlite"),
            manifest: r.join("manifest.json"),
            merchants: r.join("merchants.json"),
            x402_merchants: r.join("x402.json"),
            bind: "127.0.0.1:0".into(),
            openrouter_key: None,
            tavily_key: None,
            inference_daily_micros: 0,
        };
        (dir, AppState::new(config).unwrap())
    }
    fn quote(id: &str, n: u128, timestamp: i64) -> Quote {
        let merchant = Merchant {
            id: "reports".into(),
            resource_url: "https://reports.example/task".into(),
            facilitator_url: "https://pay.example".into(),
            recipient: "0x2222222222222222222222222222222222222222".into(),
            network: x402::NETWORK.into(),
            asset: x402::USDT.into(),
            fee_payer: String::new(),
            max_amount: 100,
        };
        let offer = Offer {
            resource: x402::Resource {
                url: merchant.resource_url.clone(),
                description: None,
                mime_type: None,
            },
            accepted: x402::Requirements {
                scheme: "exact".into(),
                network: x402::NETWORK.into(),
                amount: n.to_string(),
                asset: x402::USDT.into(),
                pay_to: merchant.recipient.clone(),
                max_timeout_seconds: 60,
                extra: json!({"assetTransferMethod":"permit2"}),
            },
            amount_units: n,
            request_hash: "fixture".into(),
            status: "requires_wallet_authorization".into(),
        };
        let authorization = Authorization::new(
            &offer,
            "0x1111111111111111111111111111111111111111",
            timestamp as u64,
        )
        .unwrap();
        Quote {
            id: identifier(),
            agent_id: id.into(),
            merchant,
            offer,
            authorization,
            day: timestamp / 86400,
            expires: timestamp + 60,
            task_hash: String::new(),
        }
    }
    #[test]
    fn concurrent_quotes_cannot_overbook_daily_budget() {
        let (_d, state) = state();
        let t = Utc::now().timestamp();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles = (0..8)
            .map(|_| {
                let s = state.clone();
                let b = barrier.clone();
                std::thread::spawn(move || {
                    let q = quote("agent", 30, t);
                    b.wait();
                    s.reserve_x402_quote("owner", &q, 100, 0, t).is_ok()
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            handles
                .into_iter()
                .filter_map(|h| h.join().ok())
                .filter(|ok| *ok)
                .count(),
            3
        );
    }
    #[test]
    fn unknown_submissions_keep_reservation_after_expiry() {
        let (_d, s) = state();
        let t = Utc::now().timestamp();
        let q = quote("agent", 70, t);
        s.reserve_x402_quote("owner", &q, 100, 0, t).unwrap();
        s.store
            .connect()
            .unwrap()
            .execute(
                "UPDATE x402_quotes SET status='uncertain' WHERE id=?",
                [q.id],
            )
            .unwrap();
        let later = quote("agent", 40, t + 70);
        assert!(s
            .reserve_x402_quote("owner", &later, 100, 0, t + 70)
            .is_err());
    }
    #[test]
    fn unsigned_expired_quotes_release_budget_but_chain_spend_counts() {
        let (_d, s) = state();
        let t = 1000;
        let q = quote("agent", 70, t);
        s.reserve_x402_quote("owner", &q, 100, 0, t).unwrap();
        s.reserve_x402_quote("owner", &quote("agent", 60, t + 70), 100, 0, t + 70)
            .unwrap();
        assert!(s
            .reserve_x402_quote("owner", &quote("agent", 20, t + 70), 100, 30, t + 70)
            .is_err());
    }
    #[test]
    fn history_is_owner_scoped_and_never_returns_signatures() {
        let (_d, s) = state();
        let input=serde_json::from_value(json!({"name":"willow","purpose":"research published protocol sources","tools":["bnb-rpc"],"daily_cap":"10","max_call":"1"})).unwrap();
        let a = s.create_agent("owner", input).unwrap();
        let t = Utc::now().timestamp();
        let q = quote(&a.id, 30, t);
        s.reserve_x402_quote("owner", &q, 100, 0, t).unwrap();
        s.store
            .connect()
            .unwrap()
            .execute(
                "UPDATE x402_quotes SET signature='sensitive authorization' WHERE id=?",
                [q.id],
            )
            .unwrap();
        let h = s.x402_history("owner", &a.id).unwrap();
        assert_eq!(h.as_array().unwrap().len(), 1);
        assert!(!h.to_string().contains("sensitive"));
        assert!(!h.to_string().contains("signature"));
        assert!(s.x402_history("intruder", &a.id).is_err());
    }
    #[test]
    fn sending_quotes_is_serialized_per_agent() {
        let (_d, s) = state();
        let t = Utc::now().timestamp();
        let a = quote("agent", 30, t);
        let b = quote("agent", 30, t);
        s.reserve_x402_quote("owner", &a, 100, 0, t).unwrap();
        s.reserve_x402_quote("owner", &b, 100, 0, t).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles = [a, b]
            .into_iter()
            .map(|q| {
                let state = s.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    state
                        .claim_x402_quote("owner", &q, "signature", t, 100, 0)
                        .is_ok()
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            handles
                .into_iter()
                .filter_map(|h| h.join().ok())
                .filter(|s| *s)
                .count(),
            1
        );
    }
    #[test]
    fn changed_policy_and_chain_spending_block_prepared_quotes() {
        let (_d, s) = state();
        let t = Utc::now().timestamp();
        let a = quote("agent", 60, t);
        s.reserve_x402_quote("owner", &a, 100, 0, t).unwrap();
        assert!(s
            .claim_x402_quote("owner", &a, "signature", t, 50, 0)
            .is_err());
        assert!(s
            .claim_x402_quote("owner", &a, "signature", t, 100, 41)
            .is_err());
        assert!(s
            .claim_x402_quote("owner", &a, "signature", t, 100, 40)
            .is_ok());
    }
    #[test]
    fn changed_task_blocks_submission_before_authorization_is_saved() {
        let (_d, s) = state();
        let input=serde_json::from_value(json!({"name":"willow","purpose":"read actual market sources","tools":["x402"],"daily_cap":"1","max_call":"0.1"})).unwrap();
        let mut agent = s.create_agent("owner", input).unwrap();
        let t = Utc::now().timestamp();
        let mut q = quote(&agent.id, 30, t);
        q.task_hash = s.policy(&agent);
        s.reserve_x402_quote("owner", &q, 100, 0, t).unwrap();
        agent.plan.purpose = "an unrelated new task".into();
        s.store.save_agent(&agent).unwrap();
        assert!(s
            .claim_x402_quote("owner", &q, "private authorization", t, 100, 0)
            .is_err());
        let (status, signature): (String, Option<String>) = s
            .store
            .connect()
            .unwrap()
            .query_row(
                "SELECT status,signature FROM x402_quotes WHERE id=?",
                [q.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "prepared");
        assert!(signature.is_none());
    }
    #[test]
    fn payment_history_keeps_delivery_after_reload_without_exposing_authorization() {
        let (_d, s) = state();
        let input=serde_json::from_value(json!({"name":"willow","purpose":"read actual market sources","tools":["x402"],"daily_cap":"1","max_call":"0.1"})).unwrap();
        let agent = s.create_agent("owner", input).unwrap();
        let t = Utc::now().timestamp();
        let mut q = quote(&agent.id, 30, t);
        q.task_hash = s.policy(&agent);
        s.reserve_x402_quote("owner", &q, 100, 0, t).unwrap();
        s.claim_x402_quote("owner", &q, "private authorization", t, 100, 0)
            .unwrap();
        let hash = "0x1111111111111111111111111111111111111111111111111111111111111111";
        s.capture_paid_delivery(
            "owner",
            &agent.id,
            &q.id,
            hash,
            br#"{"market":"actual returned data"}"#,
            true,
        )
        .unwrap();
        let before = s.x402_history("owner", &agent.id).unwrap();
        assert_eq!(before[0]["delivery"]["status"], "pending_payment");
        let db = s.store.connect().unwrap();
        db.execute(
            "UPDATE x402_quotes SET status='confirmed' WHERE id=?",
            [&q.id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO chain_receipts VALUES(?,?,?)",
            params![hash, q.id, now()],
        )
        .unwrap();
        let reopened = AppState::new((*s.config).clone()).unwrap();
        let history = reopened.x402_history("owner", &agent.id).unwrap();
        assert_eq!(history[0]["delivery"]["status"], "completed");
        assert_eq!(
            history[0]["delivery"]["data"]["market"],
            "actual returned data"
        );
        assert!(!history.to_string().contains("private authorization"));
        assert!(!history.to_string().contains("signature"));
        assert!(reopened.x402_history("intruder", &agent.id).is_err());
        let mut legacy = serde_json::to_value(&q).unwrap();
        legacy.as_object_mut().unwrap().remove("task_hash");
        let decoded: Quote = serde_json::from_value(legacy).unwrap();
        assert!(decoded.task_hash.is_empty());
    }
}
