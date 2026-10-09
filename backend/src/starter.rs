//! Public, read-only starter observations. These never create agents or activity.
use crate::{
    bnb,
    error::{ApiError, Result},
    models::*,
    AppState,
};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use utoipa::ToSchema;

#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct StarterObservation {
    pub network: String,
    pub chain_id: u64,
    pub wallet: String,
    pub bnb: String,
    pub usdt: String,
    pub block: String,
    pub observed_at: String,
    pub source: String,
    pub settled_usdt: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::Store, tests::state};
    use rust_decimal::Decimal;

    fn input(public: bool) -> AgentInput {
        serde_json::from_value(json!({"name":"receipt wren","purpose":"Read a wallet balance","tools":["bnb-rpc"],"daily_cap":"1","max_call":"0.1","public_activity":public})).unwrap()
    }
    fn registered(state: &AppState, owner: &str, public: bool) -> RuntimeAgent {
        let mut agent = state.create_agent(owner, input(public)).unwrap();
        agent.registry_id = Some("0x1111111111111111111111111111111111111111".into());
        agent.wallet = Some("0x2222222222222222222222222222222222222222".into());
        agent.status = "ready".into();
        state.store.save_agent(&agent).unwrap();
        agent
    }
    #[test]
    fn public_receipts_require_registration_public_consent_and_matching_run() {
        let (_dir, state) = state();
        let public = registered(&state, "a", true);
        let private = registered(&state, "b", false);
        let draft = state.create_agent("a", input(true)).unwrap();
        let db = state.store.connect().unwrap();
        for (id, agent) in [
            ("public-run", &public),
            ("private-run", &private),
            ("draft-run", &draft),
        ] {
            db.execute("INSERT INTO agent_runs VALUES(?,?,?,?,?,?)",params![id,agent.id,"partial","2026-10-08T10:00:00Z","2026-10-08T10:01:00Z",json!({"openrouter":{"summary":"public result","prompt":"private prompt","api_key":"private key"},"private_blob":"do not publish"}).to_string()]).unwrap();
        }
        let receipt = state.public_run_receipt(&public.id, "public-run").unwrap();
        assert_eq!(receipt["status"], "partial");
        assert_eq!(receipt["output"]["summary"], "public result");
        assert!(!receipt.to_string().contains("private prompt"));
        assert!(!receipt.to_string().contains("private key"));
        assert!(!receipt.to_string().contains("private_blob"));
        assert!(state.public_run_receipt(&public.id, "private-run").is_err());
        assert!(state
            .public_run_receipt(&private.id, "private-run")
            .is_err());
        assert!(state.public_run_receipt(&draft.id, "draft-run").is_err());
    }
    #[tokio::test]
    async fn operator_records_exclude_unfunded_acceptance_and_count_repeat_root_buyers() {
        let (_dir, state) = state();
        let buyer = registered(&state, "a", true);
        let worker = registered(&state, "b", true);
        for (funding, proof) in [
            ("funded", true),
            ("funded", true),
            ("unfunded", true),
            ("funded", false),
        ] {
            let plan:JobInput=serde_json::from_value(json!({"title":"wallet report","description":"Read a wallet and provide its observed balances","executor_id":worker.id,"budget":"1","max_call":"0.1","tools":["bnb-rpc"],"deadline":(Utc::now()+chrono::Duration::hours(4)).to_rfc3339(),"public_activity":true})).unwrap();
            let mut job = state.create_job("a", &buyer.id, plan, None).await.unwrap();
            job.state = "accepted".into();
            job.funding = funding.into();
            job.reward_paid = Decimal::new(98, 2);
            job.chain_tx = proof.then(|| format!("0x{}", "a".repeat(64)));
            Store::save_job(&state.store.connect().unwrap(), &job).unwrap();
        }
        let records = state.operator_records().unwrap();
        let worker = records["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["agent"]["id"] == worker.id)
            .unwrap();
        assert_eq!(worker["accepted_root_jobs"], 2);
        assert_eq!(worker["repeat_buyers"], 1);
        assert_eq!(worker["executor_payments_usdt"], "1.96");
    }
}
static OBSERVATIONS: OnceLock<Mutex<HashMap<String, (Instant, StarterObservation)>>> =
    OnceLock::new();
static READS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);
impl AppState {
    pub async fn starter_wallet(&self, wallet: &str) -> Result<StarterObservation> {
        let wallet = bnb::address(wallet)?;
        if wallet == bnb::ZERO {
            return Err(ApiError::validation(
                "Choose a wallet address with an owner.",
            ));
        }
        let key = format!("{}:{wallet}", self.config.rpc);
        let cache = OBSERVATIONS.get_or_init(|| Mutex::new(HashMap::new()));
        if let Some((at, value)) = cache.lock().map_err(|_| ApiError::internal())?.get(&key) {
            if at.elapsed() < Duration::from_secs(20) {
                return Ok(value.clone());
            }
        }
        let _permit = READS
            .try_acquire()
            .map_err(|_| ApiError::unavailable("Wallet reads are busy. Retry shortly."))?;
        self.bnb.require_network().await?;
        let block = bnb::number(&self.bnb.rpc("eth_blockNumber", json!([])).await?)?.to_string();
        let balance = self.bnb.balances(&wallet).await?;
        let value = StarterObservation {
            network: "BNB Smart Chain".into(),
            chain_id: 56,
            wallet,
            bnb: balance["bnb"]
                .as_str()
                .ok_or_else(ApiError::internal)?
                .into(),
            usdt: balance["usdt"]
                .as_str()
                .ok_or_else(ApiError::internal)?
                .into(),
            block,
            observed_at: Utc::now().to_rfc3339(),
            source: "public_rpc".into(),
            settled_usdt: false,
        };
        let mut values = cache.lock().map_err(|_| ApiError::internal())?;
        values.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(20));
        if values.len() >= 128 {
            values.clear();
        }
        values.insert(key, (Instant::now(), value.clone()));
        Ok(value)
    }

    pub fn public_run_receipt(&self, agent_id: &str, run_id: &str) -> Result<Value> {
        let db = self.store.connect()?;
        let raw: Option<String> = db.query_row("SELECT payload FROM runtime_agents WHERE id=? AND json_extract(payload,'$.public_activity')=1 AND json_extract(payload,'$.registry_id') IS NOT NULL", [agent_id], |r| r.get(0)).optional()?;
        let agent: RuntimeAgent = raw
            .map(|v| serde_json::from_str(&v))
            .transpose()?
            .ok_or_else(|| ApiError::missing("Public agent not found."))?;
        let row: Option<(String,String,Option<String>,String)> = db.query_row("SELECT status,started_at,finished_at,output FROM agent_runs WHERE id=? AND agent_id=?",params![run_id,agent_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let (status, started, finished, raw) =
            row.ok_or_else(|| ApiError::missing("Public run not found."))?;
        let output: Value = serde_json::from_str(&raw)?;
        let events = self
            .events(None, Some(agent_id), 0, 500)?
            .into_iter()
            .filter(|e| {
                e.timestamp >= started && finished.as_ref().is_none_or(|end| &e.timestamp <= end)
            })
            .collect::<Vec<_>>();
        Ok(
            json!({"id":run_id,"agent_id":agent_id,"agent":agent.plan.name,"task":agent.plan.purpose,"status":status,"started_at":started,"finished_at":finished,"tools":agent.plan.tools,"output":crate::runtime::safe_preview(&output),"events":events,"cost_accounting":"provider USD costs and confirmed USDT transfers are reported separately"}),
        )
    }

    pub fn operator_records(&self) -> Result<Value> {
        let agents = self.public_agents()?;
        let jobs = self.public_jobs()?;
        let mut records = vec![];
        for agent in agents {
            let mut buyers: HashMap<String, u64> = HashMap::new();
            let mut gross = rust_decimal::Decimal::ZERO;
            let mut accepted = 0u64;
            let mut cancelled = 0u64;
            for job in jobs
                .iter()
                .filter(|j| j.executor_id == agent.id && j.parent_id.is_none())
            {
                if job.state == "accepted" && job.funding == "funded" && job.chain_tx.is_some() {
                    accepted += 1;
                    gross += job.reward_paid;
                    *buyers.entry(job.requester_id.clone()).or_default() += 1;
                } else if job.state == "cancelled" && job.chain_tx.is_some() {
                    cancelled += 1;
                }
            }
            records.push(json!({"agent":agent,"accepted_root_jobs":accepted,"cancelled_root_jobs":cancelled,"distinct_buyers":buyers.len(),"repeat_buyers":buyers.values().filter(|n|**n>1).count(),"executor_payments_usdt":gross.to_string(),"scope":"public root jobs with verified chain records; buyer identifiers do not establish independent customers"}));
        }
        Ok(json!({"source":"verified_public_records","records":records}))
    }
}
