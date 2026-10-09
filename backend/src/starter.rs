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

    #[tokio::test]
    async fn unlisted_team_agents_disappear_from_public_routes_without_losing_owned_records() {
        use axum::{
            body::Body,
            http::{Request, StatusCode},
        };
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let (_dir, state) = state();
        let mut hidden_buyer = registered(&state, "owner", true);
        let mut hidden_worker = registered(&state, "owner", true);
        let mut hidden_registry = registered(&state, "owner", true);
        hidden_registry.registry_id =
            Some("0xcaaf7c62ca25081e483bf60eea68f779f218fdc21ba442408dc4aeb2da1c426b".into());
        state.store.save_agent(&hidden_registry).unwrap();
        let visible_buyer = registered(&state, "owner", true);
        let mut visible_worker = registered(&state, "owner", true);
        // Listing exclusions name exact identities, not names or owner wallets.
        visible_worker.plan.name = "tab demo finch".into();
        state.store.save_agent(&visible_worker).unwrap();
        for (agent, id) in [
            (&mut hidden_buyer, "be73320b0c6a4b0bb186f1cb77aa9aee"),
            (&mut hidden_worker, "e6ef573e424648c0b9e033324821614c"),
        ] {
            let old = agent.id.clone();
            agent.id = id.into();
            state
                .store
                .connect()
                .unwrap()
                .execute(
                    "UPDATE runtime_agents SET id=?,payload=? WHERE id=?",
                    params![id, crate::db::encode(agent).unwrap(), old],
                )
                .unwrap();
        }
        let make_job = |worker: &RuntimeAgent| {
            serde_json::from_value(json!({
            "title":"actual source report","description":"Read actual onchain observations and return the evidence",
            "executor_id":worker.id,"budget":"1","max_call":"0.1","tools":["bnb-rpc"],
            "deadline":(Utc::now()+chrono::Duration::hours(4)).to_rfc3339(),"public_activity":true,
        })).unwrap()
        };
        let hidden_job = state
            .create_job("owner", &hidden_buyer.id, make_job(&visible_worker), None)
            .await
            .unwrap();
        state
            .create_job("owner", &visible_buyer.id, make_job(&hidden_worker), None)
            .await
            .unwrap();
        let visible_job = state
            .create_job("owner", &visible_buyer.id, make_job(&visible_worker), None)
            .await
            .unwrap();
        let mut bounty=state.create_bounty("owner",serde_json::from_value(json!({"title":"read actual onchain state","description":"Return real chain observations for review","budget":"1","deadline":(Utc::now()+chrono::Duration::hours(4)).to_rfc3339(),"tools":["bnb-rpc"],"public_activity":true})).unwrap()).unwrap();
        bounty.assigned_agent_id = Some(hidden_worker.id.clone());
        bounty.job_id = Some(hidden_job.id.clone());
        state
            .store
            .connect()
            .unwrap()
            .execute(
                "UPDATE bounties SET payload=? WHERE id=?",
                params![crate::db::encode(&bounty).unwrap(), bounty.id],
            )
            .unwrap();
        let private_payload =
            crate::db::encode(&state.store.agent("owner", &hidden_buyer.id).unwrap()).unwrap();
        let policy = state.policy(&hidden_buyer);
        let db = state.store.connect().unwrap();
        let hash = format!("0x{}", "a".repeat(64));
        state
            .store
            .claim_receipt(&hash, "team-paid-resource")
            .unwrap();
        state
            .event(
                &hidden_buyer,
                "payment",
                "verified USDT payment",
                "confirmed",
                Some("market-data"),
                Some("0.000001"),
                Some("USDT"),
                Some(&hash),
            )
            .unwrap();
        for agent in [
            &hidden_buyer,
            &hidden_worker,
            &hidden_registry,
            &visible_worker,
        ] {
            db.execute(
                "INSERT INTO agent_runs VALUES(?,?,?,?,?,?)",
                params![
                    format!("run-{}", agent.id),
                    agent.id,
                    "completed",
                    "2026-10-09T01:00:00Z",
                    "2026-10-09T01:00:01Z",
                    json!({"openrouter":{"summary":"actual stored report"}}).to_string()
                ],
            )
            .unwrap();
            state
                .event(
                    agent,
                    "run_completed",
                    "agent run finished",
                    "completed",
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
        }
        let private_events_before = serde_json::to_value(
            state
                .events(Some("owner"), Some(&hidden_buyer.id), 0, 100)
                .unwrap(),
        )
        .unwrap();
        for agent in [&hidden_buyer, &hidden_worker] {
            db.execute(
                "INSERT INTO agent_public_exclusions VALUES(?,?)",
                params![agent.id, now()],
            )
            .unwrap();
        }
        db.execute(
            "INSERT INTO registry_public_exclusions VALUES(?,?)",
            params![hidden_registry.registry_id, now()],
        )
        .unwrap();
        let router = crate::api::router(state.clone());
        for path in [
            "/api/agents/live",
            "/api/agents/records",
            "/api/activity",
            "/api/feed",
            "/api/operators",
            "/api/jobs",
            "/api/bounties",
            "/api/metrics",
        ] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let text = String::from_utf8(
                response
                    .into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .to_vec(),
            )
            .unwrap();
            assert!(!text.contains(&hidden_buyer.id), "{path}");
            assert!(!text.contains(&hidden_worker.id), "{path}");
            assert!(!text.contains(&hidden_registry.id), "{path}");
        }
        for agent in [&hidden_buyer, &hidden_worker, &hidden_registry] {
            for path in [
                format!("/api/agents/live/{}", agent.id),
                format!("/api/agents/live/{}/runs/run-{}", agent.id, agent.id),
            ] {
                let response = router
                    .clone()
                    .oneshot(Request::builder().uri(&path).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
            }
        }
        let visible = state.public_agents().unwrap();
        assert!(visible
            .iter()
            .any(|a| a.id == visible_worker.id && a.name == "tab demo finch"));
        assert_eq!(
            state
                .public_jobs()
                .unwrap()
                .iter()
                .map(|j| j.id.as_str())
                .collect::<Vec<_>>(),
            vec![visible_job.id.as_str()]
        );
        assert_eq!(state.store.runs(&hidden_buyer.id).unwrap().len(), 1);
        assert_eq!(
            serde_json::to_value(
                state
                    .events(Some("owner"), Some(&hidden_buyer.id), 0, 100)
                    .unwrap()
            )
            .unwrap(),
            private_events_before
        );
        assert!(state
            .events(Some("intruder"), Some(&hidden_buyer.id), 0, 100)
            .unwrap()
            .is_empty());
        assert_eq!(
            crate::db::encode(&state.store.agent("owner", &hidden_buyer.id).unwrap()).unwrap(),
            private_payload
        );
        assert_eq!(
            state.policy(&state.store.agent("owner", &hidden_buyer.id).unwrap()),
            policy
        );
        assert_eq!(
            state.get_job("owner", &hidden_job.id).unwrap().id,
            hidden_job.id
        );
        assert!(state.bounties(None).unwrap().is_empty());
        assert_eq!(state.bounties(Some("owner")).unwrap().len(), 1);
        assert_eq!(
            db.query_row(
                "SELECT action_id FROM chain_receipts WHERE tx_hash=?",
                [hash],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "team-paid-resource"
        );
        let third_party = registered(&state, "another-owner", true);
        assert!(state
            .create_job(
                "another-owner",
                &third_party.id,
                make_job(&hidden_worker),
                None
            )
            .await
            .is_err());
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
        let raw: Option<String> = db.query_row("SELECT a.payload FROM runtime_agents a WHERE a.id=? AND json_extract(a.payload,'$.public_activity')=1 AND json_extract(a.payload,'$.registry_id') IS NOT NULL AND NOT EXISTS(SELECT 1 FROM unlisted_public_agents x WHERE x.agent_id=a.id)", [agent_id], |r| r.get(0)).optional()?;
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
