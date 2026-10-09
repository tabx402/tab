use crate::{
    bnb,
    db::{encode, Store},
    error::{ApiError, Result},
    models::*,
    AppState,
};
use chrono::{Duration, Utc};
use rusqlite::params;
use rust_decimal::Decimal;
use serde_json::{json, Value};

impl AppState {
    pub async fn providers(&self) -> Vec<Provider> {
        let x402_ready = self
            .x402_system()
            .await
            .is_ok_and(|s| s["settlement_enabled"] == true);
        vec![
            Provider {
                id: "bnb-rpc".into(),
                name: "BNB Smart Chain".into(),
                category: "data".into(),
                description:
                    "Live BNB and USDT balances, confirmed blocks and transaction evidence.".into(),
                website: "https://docs.bnbchain.org/".into(),
                status: "live".into(),
            },
            Provider {
                id: "openrouter".into(),
                name: "OpenRouter".into(),
                category: "inference".into(),
                description:
                    "Choose from approved language models under per-call and daily spending limits."
                        .into(),
                website: "https://openrouter.ai/".into(),
                status: if self.config.openrouter_key.is_some() && self.config.inference_daily_micros > 0 {
                    "live"
                } else {
                    "not_connected"
                }
                .into(),
            },
            Provider {
                id: "tavily".into(),
                name: "Tavily".into(),
                category: "data".into(),
                description: "Web search returns source links and excerpts for research reports."
                    .into(),
                website: "https://www.tavily.com/".into(),
                status: if self.config.tavily_key.is_some() && self.config.inference_daily_micros > 0 {
                    "live"
                } else {
                    "not_connected"
                }
                .into(),
            },
            Provider {
                id: "x402".into(),
                name: "x402".into(),
                category: "data".into(),
                description:
                    "USDT market-data requests on BNB Smart Chain. Review a price and approve the exact payment in your wallet.".into(),
                website: "https://www.x402.org/".into(),
                status: if x402_ready { "live" } else { "not_connected" }.into(),
            },
            Provider {
                id: "web-search".into(),
                name: "web research".into(),
                category: "data".into(),
                description: "Source-linked research through OpenRouter. Provider credits are tracked separately from USDT payments.".into(),
                website: "https://openrouter.ai/docs/guides/features/plugins/web-search".into(),
                status: if self.config.inference_daily_micros > 0 && (self.config.openrouter_key.is_some() || self.config.tavily_key.is_some()) { "live" } else { "not_connected" }.into(),
            },
            Provider {
                id: "anthropic".into(),
                name: "Anthropic".into(),
                category: "inference".into(),
                description: "Claude models are available through the configured OpenRouter route."
                    .into(),
                website: "https://www.anthropic.com/".into(),
                status: "candidate".into(),
            },
            Provider {
                id: "dune".into(),
                name: "Dune".into(),
                category: "data".into(),
                description:
                    "Onchain datasets require a separately configured provider connection.".into(),
                website: "https://dune.com/".into(),
                status: "not_connected".into(),
            },
        ]
    }
    pub async fn model_directory(&self) -> Value {
        let remote = match self
            .client
            .get("https://openrouter.ai/api/v1/models")
            .timeout(std::time::Duration::from_secs(8))
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    response.json::<Value>().await.ok()
                } else {
                    None
                }
            }
            Err(_) => None,
        };
        let data = remote.as_ref().and_then(|v| v["data"].as_array());
        json!(MODELS.iter().map(|id|{
            let entry=data.and_then(|entries|entries.iter().find(|entry|entry["id"]==*id));
            let provider=id.split('/').next().unwrap_or("model");
            let available=self.config.openrouter_key.is_some()&&self.config.inference_daily_micros>0&&entry.is_some();
            json!({"id":id,"name":entry.and_then(|e|e["name"].as_str()).unwrap_or(id),"provider":provider,"available":available,"status":if available{"connected"}else{"not_connected"},"catalog_verified":entry.is_some(),"billing":"development credits","max_output_tokens":256})
        }).collect::<Vec<_>>())
    }
    fn empty_registry(&self) -> RegistryData {
        RegistryData {
            status: if self.config.program.is_empty() {
                "not_deployed"
            } else {
                "checking"
            }
            .into(),
            chain_id: 56,
            network: "mainnet".into(),
            address: None,
            owner: None,
            block_number: None,
            verified_at: None,
            agents: vec![],
            transactions: vec![],
            fees_bnb: None,
            error: None,
            discovery: self.bnb.registry_progress(&self.store),
        }
    }
    pub async fn registry(&self) -> RegistryData {
        // Public traffic reads a background-verified snapshot. It never queues
        // behind historical log scans or launches one RPC per registered agent.
        let discovery = self.bnb.registry_progress(&self.store);
        let cached = self
            .registry_cache
            .read()
            .await
            .clone()
            .filter(|snapshot| snapshot.generation == self.bnb.registry_generation());
        let mut registry = cached
            .as_ref()
            .map(|snapshot| snapshot.data.clone())
            .unwrap_or_else(|| self.empty_registry());
        if !self.config.program.is_empty() {
            registry.status = if cached.is_none() && discovery.status == "live" {
                "checking".into()
            } else if cached.is_some()
                && discovery.status == "live"
                && registry.block_number != discovery.scanned_through_block
            {
                "stale".into()
            } else {
                discovery.status.clone()
            };
        }
        // A known checkpoint reorg invalidates all hydrated records immediately.
        // A temporary outage preserves the last verified snapshot as stale.
        if self.bnb.cached_registered_ids().is_none() {
            registry.agents.clear();
            registry.owner = None;
            registry.block_number = None;
            registry.verified_at = None;
        }
        registry.discovery = discovery;
        registry
    }
    pub async fn refresh_registry(&self) -> Result<()> {
        if self.config.program.is_empty() {
            return Ok(());
        }
        // Bound background hydration as well as scanning. The scanner commits
        // complete verified chunks, so a timeout resumes at durable progress.
        tokio::time::timeout(
            std::time::Duration::from_secs(12),
            self.refresh_registry_snapshot(),
        )
        .await
        .map_err(|_| ApiError::unavailable("BNB registry hydration timed out."))?
    }
    async fn refresh_registry_snapshot(&self) -> Result<()> {
        if !self.bnb.deployed().await {
            return Err(ApiError::unavailable(
                "BNB deployment verification is reconnecting.",
            ));
        }
        let scanned = self.bnb.registered_ids(&self.store).await;
        let Some((ids, confirmed)) = self.bnb.cached_registered_ids() else {
            return scanned.map(|_| ());
        };
        let generation = self.bnb.registry_generation();
        let discovery = self.bnb.registry_progress(&self.store);
        let mut registry = self.empty_registry();
        let block = format!("0x{confirmed:x}");
        for id in ids {
            let a = self
                .bnb
                .view_at("protocol", "getAgent", vec![bnb::bytes32(&id)?], &block)
                .await?;
            if a["owner"] == bnb::ZERO {
                continue;
            }
            registry.agents.push(RegisteredAgent {
                id,
                name: a["name"].as_str().unwrap_or_default().into(),
                purpose: "Registered BNB agent policy".into(),
                owner: a["owner"].as_str().unwrap_or_default().into(),
                daily_cap: money(bnb::number(&a["dailyCap"])?),
                providers: vec![],
                paused: a["paused"] == true,
                version: bnb::number(&a["version"])? as u32,
                policy_hash: a["policyHash"].as_str().unwrap_or_default().into(),
                policy_matches: false,
                funding_status: "not_assessed".into(),
            });
        }
        registry.owner = self
            .bnb
            .view_at("protocol", "authority", vec![], &block)
            .await?
            .as_str()
            .map(str::to_owned);
        // The chain can reorganize during hydration. Publishing only an atomic
        // snapshot anchored to its canonical block avoids mixing histories.
        self.bnb.verify_registry_block(confirmed).await?;
        if generation != self.bnb.registry_generation() {
            return Err(ApiError::unavailable(
                "BNB registry history changed during hydration.",
            ));
        }
        registry.block_number = Some(confirmed.to_string());
        registry.status = discovery.status.clone();
        registry.address = Some(self.config.program.clone());
        registry.verified_at = discovery.verified_at.clone();
        registry.discovery = discovery;
        *self.registry_cache.write().await = Some(crate::registry_cache::RegistrySnapshot {
            data: registry,
            generation,
        });
        // Catch-up is an expected background state; completed chunks are useful
        // now, and the next interval continues through the remaining history.
        match scanned {
            Err(error)
                if error.1 == "Confirmed registry history is catching up. Refresh shortly." =>
            {
                Ok(())
            }
            other => other.map(|_| ()),
        }
    }
    pub fn feed(&self) -> Result<Value> {
        let events = self.events(None, None, 0, 100)?;
        // Publish only action receipts; private prompts and model deliberation never enter the feed.
        Ok(
            json!({"mode":"public","items":events,"agents":self.public_agents()?,"jobs":self.public_jobs()?,"network":self.config.network}),
        )
    }
    pub fn create_bounty(&self, owner: &str, mut plan: BountyInput) -> Result<Bounty> {
        plan.title = clean(&plan.title, 3, 120)?;
        plan.description = clean(&plan.description, 5, 1200)?;
        if plan.budget <= Decimal::ZERO || plan.budget > Decimal::from(10000) {
            return Err(ApiError::validation(
                "Use a bounty budget from zero to 10,000 USDT.",
            ));
        }
        units(plan.budget)?;
        if plan.deadline < Utc::now() + Duration::minutes(5)
            || plan.deadline > Utc::now() + Duration::days(30)
            || plan.tools.is_empty()
            || plan.tools.len() > 5
            || !distinct(&plan.tools)
            || plan.tools.iter().any(|t| !TOOLS.contains(&t.as_str()))
        {
            return Err(ApiError::validation(
                "Choose supported tools and a deadline within thirty days.",
            ));
        }
        let bounty = Bounty {
            plan,
            id: identifier(),
            created_at: now(),
            status: "open_draft".into(),
            funding: "unfunded".into(),
            assigned_agent_id: None,
            job_id: None,
            chain_tx: None,
        };
        let mut db = self.store.connect()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let count: u64 = tx.query_row(
            "SELECT count(*) FROM bounties WHERE owner=? AND created_at>?",
            params![owner, (Utc::now() - Duration::days(1)).to_rfc3339()],
            |r| r.get(0),
        )?;
        if count >= 20 {
            return Err(ApiError::conflict("Daily bounty creation limit reached."));
        }
        tx.execute(
            "INSERT INTO bounties VALUES(?,?,?,?)",
            params![bounty.id, owner, encode(&bounty)?, bounty.created_at],
        )?;
        tx.commit()?;
        Ok(bounty)
    }
    pub fn bounties(&self, owner: Option<&str>) -> Result<Vec<Bounty>> {
        let db = self.store.connect()?;
        if let Some(owner) = owner {
            Store::list_payload(
                &db,
                "SELECT payload FROM bounties WHERE owner=? ORDER BY created_at DESC LIMIT 200",
                [owner],
            )
        } else {
            Store::list_payload(&db,"SELECT payload FROM bounties WHERE json_extract(payload,'$.public_activity')=1 ORDER BY created_at DESC LIMIT 200",[])
        }
    }
    pub async fn protocol_data(&self) -> Result<Option<Value>> {
        if !self.bnb.deployed().await {
            return Ok(None);
        }
        Ok(Some(
            self.bnb.view("protocol", "getProtocol", vec![]).await?,
        ))
    }
    pub async fn token_system(&self) -> Value {
        let protocol = self.protocol_data().await.ok().flatten();
        let fee = protocol.as_ref().and_then(|p| bnb::number(&p["feeBps"]).ok());
        let configured = self
            .config
            .official_tab
            .as_ref()
            .is_some_and(|token| protocol.as_ref().is_some_and(|p| p["tabToken"] == *token));
        json!({"official_tab_address":self.config.official_tab,"status":if configured{"verified"}else if self.config.official_tab.is_none(){"not_configured"}else{"unavailable"},"staking_enabled":configured,"paired_token_required":true,"tab_holding_required":true,"fee_destination":"protocol_reserve","fee_bps":fee,"holder_fee_bps":0,"holder_exemption_enabled":configured&&fee==Some(50),"holder_exemption_scope":"executor wallet balance at settlement","fee_status":if fee.is_some(){"immutable_onchain"}else{"unverified"},"penalty_rule":"deadline_missed","buyer_disagreement_slashable":false,"token_bond_guarantees_usdt":false,"trading_pause_scope":"Tab actions only; external pools remain independent"})
    }
    pub async fn eligibility(&self, owner: &str, id: &str) -> Result<Value> {
        let agent = self.store.agent(owner, id)?;
        let system = self.token_system().await;
        if system["status"] != "verified" {
            return Ok(
                json!({"eligible":false,"reason":"The official TAB contract is not verified.","tab_holding":null,"paired_holding":null,"source":"not_verified"}),
            );
        }
        let wallet = agent
            .wallet
            .as_deref()
            .ok_or_else(|| ApiError::conflict("Register the agent first."))?;
        let agent_key = agent
            .registry_id
            .as_deref()
            .ok_or_else(|| ApiError::conflict("Register the agent first."))?;
        let launch = self
            .bnb
            .view("economics", "getLaunch", vec![bnb::bytes32(agent_key)?])
            .await?;
        let token = launch["token"]
            .as_str()
            .filter(|t| *t != bnb::ZERO)
            .ok_or_else(|| {
                ApiError::conflict("Pair an agent token before applying for token-gated work.")
            })?;
        if launch["agent"] != agent_key || launch["creator"] != wallet {
            return Err(ApiError::unavailable(
                "The agent token pair does not match this wallet.",
            ));
        }
        let (tab, td) = self
            .bnb
            .token_balance(
                wallet,
                self.config.official_tab.as_deref().expect("verified TAB"),
            )
            .await?;
        let (paired, pd) = self.bnb.token_balance(wallet, token).await?;
        Ok(
            json!({"eligible":tab>0&&paired>0,"reason":if tab>0&&paired>0{"Both required token holdings are verified."}else{"Hold TAB and the agent token to participate."},"tab_holding":{"address":self.config.official_tab,"raw_amount":tab.to_string(),"decimals":td},"paired_holding":{"address":token,"raw_amount":paired.to_string(),"decimals":pd},"source":"BNB mainnet RPC","verified_at":now()}),
        )
    }
    pub async fn metrics(&self) -> Result<Value> {
        let protocol = self.protocol_data().await?;
        let jobs = self.public_jobs()?;
        let jobs = if protocol.is_some() { jobs } else { vec![] };
        let completed = jobs
            .iter()
            .filter(|j| j.state == "accepted" && j.funding == "funded")
            .count();
        let missed = jobs
            .iter()
            .filter(|j| {
                j.state == "cancelled"
                    && j.funding == "funded"
                    && !j.timely_submitted
                    && j.cancelled_at
                        .is_some_and(|timestamp| timestamp > j.deadline.timestamp() + 86400)
            })
            .count();
        let collaborations = jobs
            .iter()
            .filter(|j| {
                j.parent_id.is_some() && j.reward_paid > Decimal::ZERO && j.funding == "funded"
            })
            .count();
        let bonded: std::collections::HashSet<_> = jobs
            .iter()
            .filter(|j| {
                ["locked", "released"].contains(&j.bond_status.as_str())
                    && j.reward_paid > Decimal::ZERO
                    && j.funding == "funded"
            })
            .map(|j| &j.executor_id)
            .collect();
        let (paid, fees, spent, tokens) = if let Some(data) = &protocol {
            (
                bnb::number(&data["completedWorkUsdt"])?,
                bnb::number(&data["feesCollectedUsdt"])?,
                bnb::number(&data["buybackSpentUsdt"])?,
                bnb::number(&data["buybackTokensAcquired"])?,
            )
        } else {
            (0, 0, 0, 0)
        };
        let db = self.store.connect()?;
        let external = jobs
            .iter()
            .filter(|j| j.parent_id.is_none() && j.state == "accepted")
            .try_fold(Decimal::ZERO, |sum, public| {
                let job = Store::job(&db, &public.id)?;
                Ok::<_, ApiError>(if job.buyer_wallet != job.executor_wallet {
                    sum + job.reward_paid + job.fee_paid
                } else {
                    sum
                })
            })?;
        Ok(
            json!({"currency":"USDT","source":"verified BNB Smart Chain receipts","bonded_agents_earning_payments":bonded.len(),"completed_commitments":completed,"missed_commitments":missed,"paid_collaborations":collaborations,"customer_payments_usdt":external.to_string(),"customer_scope":"public completed root jobs with different buyer and executor wallets","completed_work_gross_usdt":money(paid).to_string(),"fees_collected_usdt":money(fees).to_string(),"buyback_usdt_spent":money(spent).to_string(),"buyback_tokens_acquired":tokens.to_string(),"settlement_status":if protocol.is_some(){"verified"}else{"not_deployed"}}),
        )
    }
    pub fn backing_assets(&self) -> Value {
        let mut assets: Vec<Value> =
            serde_json::from_str(include_str!("../config/backing-assets.json")).unwrap_or_default();
        for asset in &mut assets {
            asset["enabled"] = json!(
                asset["chain_id"] == 56
                    && asset["address"]
                        .as_str()
                        .is_some_and(|a| bnb::address(a).is_ok())
            );
            asset["network"] = json!("mainnet");
            asset["chain_id"] = json!(56);
            asset["token_standard"] = json!("BEP20");
            asset["status"] = json!(if asset["enabled"] == true {
                "configured"
            } else {
                "not_connected"
            });
        }
        assets.insert(0,json!({"symbol":"USDT","name":"Tether USD","address":self.config.usdt,"decimals":18,"token_standard":"BEP20","category":"usdt","enabled":true,"status":"configured","network":"mainnet","chain_id":56}));
        assets.insert(1,json!({"symbol":"BNB","name":"BNB","address":null,"decimals":18,"token_standard":"native","category":"native","enabled":true,"status":"configured","network":"mainnet","chain_id":56}));
        json!(assets)
    }
}
