use crate::{
    bnb::{self, Instruction},
    db::{decode, encode, Store},
    error::{ApiError, Result},
    models::*,
    AppState,
};
use chrono::{Duration, Utc};
use ethabi::Token;
use rusqlite::{params, Connection, OptionalExtension};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::collections::HashSet;

fn owns(db: &Connection, owner: &str, agent: &str) -> Result<bool> {
    Ok(db
        .query_row(
            "SELECT 1 FROM runtime_agents WHERE owner=? AND id=?",
            params![owner, agent],
            |r| r.get::<_, u8>(0),
        )
        .optional()?
        .is_some())
}
fn access(db: &Connection, owner: &str, job: &Job) -> Result<()> {
    let root = Store::job(db, &job.root_id)?;
    if owns(db, owner, &root.requester_id)?
        || owns(db, owner, &job.plan.executor_id)?
        || owns(db, owner, &job.requester_id)?
    {
        return Ok(());
    }
    let mut ancestor = job.clone();
    for _ in 0..8 {
        let Some(parent_id) = &ancestor.parent_id else {
            break;
        };
        ancestor = Store::job(db, parent_id)?;
        if owns(db, owner, &ancestor.plan.executor_id)? {
            return Ok(());
        }
    }
    Err(ApiError::missing("Job not found."))
}
fn ready_agent(db: &Connection, id: &str, owner: Option<&str>) -> Result<RuntimeAgent> {
    signing_agent(db, id, owner, false)
}
fn signing_agent(
    db: &Connection,
    id: &str,
    owner: Option<&str>,
    allow_paused: bool,
) -> Result<RuntimeAgent> {
    let (actual, agent) = Store::agent_any(db, id)?;
    if owner.is_some_and(|expected| expected != actual)
        || (owner.is_none() && !agent.plan.public_activity)
    {
        return Err(ApiError::missing("Agent not found."));
    }
    if agent.wallet.is_none()
        || agent.registry_id.is_none()
        || (agent.status != "ready" && !(allow_paused && agent.status == "paused"))
    {
        return Err(ApiError::conflict("Register or resume the agent first."));
    }
    Ok(agent)
}
fn draft_agent(db: &Connection, id: &str, owner: Option<&str>) -> Result<RuntimeAgent> {
    let (actual, agent) = Store::agent_any(db, id)?;
    if owner.is_some_and(|expected| expected != actual)
        || (owner.is_none() && !agent.plan.public_activity)
    {
        return Err(ApiError::missing("Agent not found."));
    }
    Ok(agent)
}
fn pending(db: &Connection, id: &str) -> Result<bool> {
    Ok(db.query_row("SELECT 1 FROM job_intents WHERE job_id=? AND confirmed=0 AND (tx_hash IS NOT NULL OR julianday(json_extract(payload,'$.expires_at'))>julianday(?))",params![id,now()],|r|r.get::<_,u8>(0)).optional()?.is_some())
}
impl AppState {
    pub fn merchants(&self) -> Result<Vec<JobMerchant>> {
        if !self.config.merchants.exists() {
            return Ok(vec![]);
        }
        let raw = std::fs::read(&self.config.merchants)
            .map_err(|_| ApiError::unavailable("Job merchant directory is unavailable."))?;
        let mut merchants: Vec<JobMerchant> = serde_json::from_slice(&raw)
            .map_err(|_| ApiError::unavailable("Job merchant directory is unavailable."))?;
        for merchant in &mut merchants {
            merchant.recipient = bnb::address(&merchant.recipient)?;
            merchant.service_key = bnb::address(&merchant.service_key)?;
        }
        let mut ids = HashSet::new();
        let mut keys = HashSet::new();
        if merchants.len() > 32
            || merchants.iter().any(|m| {
                !ids.insert(&m.id)
                    || !keys.insert(&m.service_key)
                    || !TOOLS.contains(&m.tool.as_str())
                    || bnb::pubkey(&m.recipient).is_err()
                    || m.recipient == bnb::ZERO
                    || m.service_key == bnb::ZERO
            })
        {
            return Err(ApiError::unavailable(
                "Job merchant configuration is invalid.",
            ));
        }
        Ok(merchants)
    }
    pub fn get_job(&self, owner: &str, id: &str) -> Result<Job> {
        let db = self.store.connect()?;
        let job = Store::job(&db, id)?;
        access(&db, owner, &job)?;
        Ok(job)
    }
    pub fn list_jobs(&self, owner: &str, agent_id: Option<&str>, scoped: bool) -> Result<Vec<Job>> {
        let db = self.store.connect()?;
        let mut stmt = db.prepare("SELECT id FROM runtime_agents WHERE owner=?")?;
        let mut owned: HashSet<String> = stmt
            .query_map([owner], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<_, _>>()?;
        if let Some(id) = agent_id {
            if !owned.contains(id) {
                return Err(ApiError::missing("Agent not found."));
            }
            if scoped {
                owned = HashSet::from([id.to_owned()]);
            }
        }
        let jobs: Vec<Job> = Store::list_payload(
            &db,
            "SELECT payload FROM jobs ORDER BY created_at DESC LIMIT 2000",
            [],
        )?;
        let roots: HashSet<_> = jobs
            .iter()
            .filter(|j| {
                (owned.contains(&j.requester_id) || owned.contains(&j.plan.executor_id))
                    && agent_id.is_none_or(|id| j.requester_id == id || j.plan.executor_id == id)
            })
            .map(|j| j.root_id.clone())
            .collect();
        let mut visible: HashSet<String> = jobs
            .iter()
            .filter(|j| {
                roots.contains(&j.root_id)
                    && (owned.contains(&j.requester_id) || owned.contains(&j.plan.executor_id))
            })
            .map(|j| j.id.clone())
            .collect();
        for _ in 0..8 {
            let descendants: Vec<_> = jobs
                .iter()
                .filter(|j| j.parent_id.as_ref().is_some_and(|id| visible.contains(id)))
                .map(|j| j.id.clone())
                .collect();
            visible.extend(descendants);
        }
        Ok(jobs
            .into_iter()
            .filter(|j| visible.contains(&j.id))
            .take(200)
            .collect())
    }
    pub fn job_actions(&self, owner: &str) -> Result<Vec<JobActionRecord>> {
        let db = self.store.connect()?;
        let mut stmt=db.prepare("SELECT payload,tx_hash,confirmed FROM job_intents WHERE owner=? ORDER BY rowid DESC LIMIT 100")?;
        let rows = stmt.query_map([owner], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, u8>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (payload, tx_hash, confirmed) = row?;
            Ok(JobActionRecord {
                intent: decode(payload)?,
                tx_hash,
                confirmed,
            })
        })
        .collect()
    }
    pub async fn create_job(
        &self,
        owner: &str,
        requester: &str,
        plan: JobInput,
        parent_id: Option<&str>,
    ) -> Result<Job> {
        let plan = plan.validate()?;
        // A proposer cannot allocate another agent's volatile tokens without its owner consent.
        let bond_balance = if plan.bond_tokens > Decimal::ZERO {
            let executor = self.store.agent(owner, &plan.executor_id)?;
            let wallet = executor
                .wallet
                .as_deref()
                .ok_or_else(|| ApiError::conflict("Register the bonded agent first."))?;
            Some(
                self.bnb
                    .token_balance(
                        wallet,
                        plan.bond_token_address
                            .as_deref()
                            .expect("validated bond token"),
                    )
                    .await?,
            )
        } else {
            None
        };
        let merchants = self.merchants()?;
        let mut db = self.store.connect()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let buyer = draft_agent(&tx, requester, Some(owner))?;
        let (executor_owner, _) = Store::agent_any(&tx, &plan.executor_id)?;
        let executor = draft_agent(
            &tx,
            &plan.executor_id,
            if executor_owner == owner {
                Some(owner)
            } else {
                None
            },
        )?;
        if plan
            .tools
            .iter()
            .any(|tool| !buyer.plan.tools.contains(tool) || !executor.plan.tools.contains(tool))
        {
            return Err(ApiError::validation(
                "Job tools must be enabled for both agents.",
            ));
        }
        if plan.max_call > buyer.plan.max_call
            || plan.max_call > executor.plan.max_call
            || plan.budget > buyer.plan.daily_cap
        {
            return Err(ApiError::validation(
                "Keep the job within both agents' configured spending limits.",
            ));
        }
        let mut parent = parent_id.map(|id| Store::job(&tx, id)).transpose()?;
        let root_services = parent
            .as_ref()
            .map(|p| p.root_services.clone())
            .unwrap_or_else(|| {
                merchants
                    .into_iter()
                    .filter(|m| plan.services.contains(&m.id))
                    .collect()
            });
        if plan.services.iter().any(|id| {
            !root_services
                .iter()
                .any(|m| &m.id == id && plan.tools.contains(&m.tool))
        }) {
            return Err(ApiError::validation(
                "Choose connected invoice services matching the job's tools.",
            ));
        }
        if let Some(parent) = &mut parent {
            access(&tx, owner, parent)?;
            if parent.plan.executor_id != requester
                || !["draft", "open"].contains(&parent.state.as_str())
            {
                return Err(ApiError::forbidden(
                    "Only the assigned executor can delegate an open job.",
                ));
            }
            let root = Store::job(&tx, &parent.root_id)?;
            if root.paused
                || parent.depth >= 8
                || plan.deadline > parent.plan.deadline
                || plan.minimum_block < parent.plan.minimum_block
                || plan.max_call > parent.plan.max_call
                || plan.tools.iter().any(|t| !parent.plan.tools.contains(t))
                || plan
                    .services
                    .iter()
                    .any(|s| !parent.plan.services.contains(s))
            {
                return Err(ApiError::validation(
                    "A branch must inherit narrower tools, limits and deadline.",
                ));
            }
            if plan.budget > parent.available {
                return Err(ApiError::conflict(
                    "This branch exceeds the parent's available budget.",
                ));
            }
            if pending(&tx, &parent.id)? {
                return Err(ApiError::conflict(
                    "Confirm the pending wallet action before adding a branch.",
                ));
            }
            parent.available -= plan.budget;
            Store::save_job(&tx, parent)?;
        }
        let today: u64 = tx.query_row(
            "SELECT count(*) FROM jobs WHERE owner=? AND created_at>?",
            params![owner, (Utc::now() - Duration::days(1)).to_rfc3339()],
            |r| r.get(0),
        )?;
        if today >= 50 {
            return Err(ApiError(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "Daily job creation limit reached.".into(),
            ));
        }
        let id = format!("{}{}", identifier(), identifier());
        let root_id = parent
            .as_ref()
            .map(|p| p.root_id.clone())
            .unwrap_or_else(|| id.clone());
        let canonical = json!({"plan":plan,"id":id,"requester_id":requester,"buyer_wallet":buyer.wallet,"executor_wallet":executor.wallet,"parent_id":parent_id,"root_id":root_id,"root_services":root_services});
        let mut job = Job {
            plan: plan.clone(),
            id: id.clone(),
            root_id,
            parent_id: parent_id.map(str::to_owned),
            requester_id: requester.into(),
            buyer_wallet: buyer.wallet.clone().unwrap_or_default(),
            executor_wallet: executor.wallet.unwrap_or_default(),
            executor_name: executor.plan.name,
            created_at: now(),
            depth: parent.as_ref().map_or(0, |p| p.depth + 1),
            terms_hash: digest(&canonical),
            state: "draft".into(),
            funding: "unfunded".into(),
            available: plan.budget,
            evidence_hash: None,
            evidence: None,
            paused: false,
            chain_tx: None,
            provider_paid: Decimal::ZERO,
            reward_paid: Decimal::ZERO,
            refunded: Decimal::ZERO,
            root_services,
            bond_status: "none".into(),
            fee_paid: Decimal::ZERO,
            timely_submitted: false,
            submitted_at: None,
            cancelled_at: None,
            terms: canonical,
        };
        if let Some((available, decimals)) = bond_balance {
            let reserved: Vec<String> = {
                let mut stmt=tx.prepare("SELECT amount FROM bond_reservations WHERE agent_id=? AND token_address=? AND status IN ('planned','locked')")?;
                let result = stmt
                    .query_map(params![plan.executor_id, plan.bond_token_address], |r| {
                        r.get(0)
                    })?
                    .collect::<std::result::Result<_, _>>()?;
                result
            };
            let reserved = reserved.iter().try_fold(Decimal::ZERO, |sum, s| {
                s.parse::<Decimal>()
                    .ok()
                    .and_then(|amount| sum.checked_add(amount))
                    .ok_or_else(ApiError::internal)
            })?;
            let available = Decimal::from(available) / Decimal::from(10u64.pow(decimals as u32));
            if plan
                .bond_tokens
                .checked_add(reserved)
                .is_none_or(|amount| amount > available)
            {
                return Err(ApiError::conflict("This token collateral is already allocated to another commitment or exceeds the wallet balance."));
            }
            tx.execute(
                "INSERT INTO bond_reservations VALUES(?,?,?,?,?,?)",
                params![
                    id,
                    plan.executor_id,
                    plan.bond_token_address,
                    plan.bond_tokens.to_string(),
                    "planned",
                    Option::<String>::None
                ],
            )?;
            job.bond_status = "planned".into();
        }
        tx.execute(
            "INSERT INTO jobs VALUES(?,?,?,?,?,?)",
            params![
                id,
                owner,
                encode(&job)?,
                job.root_id,
                parent_id,
                job.created_at
            ],
        )?;
        tx.commit()?;
        self.event(
            &buyer,
            "job_created",
            "job terms saved; USDT budget is unfunded",
            "draft",
            None,
            None,
            None,
            None,
        )?;
        Ok(job)
    }
    pub fn public_jobs(&self) -> Result<Vec<PublicJob>> {
        let db = self.store.connect()?;
        let jobs:Vec<Job>=Store::list_payload(&db,"SELECT j.payload FROM jobs j WHERE NOT EXISTS(SELECT 1 FROM jobs p LEFT JOIN runtime_agents a ON a.id=json_extract(p.payload,'$.requester_id') LEFT JOIN runtime_agents b ON b.id=json_extract(p.payload,'$.executor_id') WHERE p.root_id=j.root_id AND (coalesce(json_extract(p.payload,'$.public_activity'),0)!=1 OR coalesce(json_extract(a.payload,'$.public_activity'),0)!=1 OR coalesce(json_extract(b.payload,'$.public_activity'),0)!=1)) ORDER BY j.created_at DESC LIMIT 500",[])?;
        Ok(jobs
            .into_iter()
            .map(|j| PublicJob {
                id: j.id,
                root_id: j.root_id,
                parent_id: j.parent_id,
                requester_id: j.requester_id,
                executor_id: j.plan.executor_id,
                executor_name: j.executor_name,
                title: j.plan.title,
                acceptance: j.plan.acceptance,
                deadline: j.plan.deadline,
                budget: j.plan.budget,
                available: j.available,
                state: j.state,
                funding: j.funding,
                terms_hash: j.terms_hash,
                evidence_hash: j.evidence_hash,
                chain_tx: j.chain_tx,
                paused: j.paused,
                reward_paid: j.reward_paid,
                provider_paid: j.provider_paid,
                bond_tokens: j.plan.bond_tokens,
                bond_token_address: j.plan.bond_token_address,
                bond_status: j.bond_status,
                penalty_rule: j.plan.penalty_rule,
                fee_paid: j.fee_paid,
                timely_submitted: j.timely_submitted,
                submitted_at: j.submitted_at,
                cancelled_at: j.cancelled_at,
                penalty_bps: j.plan.penalty_bps,
            })
            .collect())
    }
    pub async fn collect_evidence(&self, owner: &str, id: &str) -> Result<Job> {
        let job = self.get_job(owner, id)?;
        if !job.plan.tools.iter().any(|t| t == "bnb-rpc") {
            return Err(ApiError::conflict(
                "Attach research outputs for buyer review, or enable BNB chain observations.",
            ));
        }
        self.bnb.require_network().await?;
        let head = bnb::number(&self.bnb.rpc("eth_blockNumber", json!([])).await?)?;
        let number = head.saturating_sub(u128::from(self.config.confirmations));
        if number < u128::from(job.plan.minimum_block) {
            return Err(ApiError::conflict(
                "A qualifying confirmed BNB block is not available yet.",
            ));
        }
        let block = self
            .bnb
            .rpc(
                "eth_getBlockByNumber",
                json!([format!("0x{number:x}"), false]),
            )
            .await?;
        let hash = block["hash"]
            .as_str()
            .ok_or_else(|| ApiError::unavailable("BNB block observation is unavailable."))?;
        bnb::signature(hash)?;
        self.attach_evidence(owner,id,json!({"chain_id":56,"network":"mainnet","block_number":number,"block_hash":hash,"observed_at":now(),"verification":"buyer_review"}))
    }
    pub fn attach_evidence(&self, owner: &str, id: &str, evidence: Value) -> Result<Job> {
        if !evidence.is_object()
            || evidence.as_object().is_none_or(|o| o.is_empty())
            || serde_json::to_vec(&evidence)?.len() > 16000
        {
            return Err(ApiError::validation(
                "Provide nonempty evidence smaller than 16 KB.",
            ));
        }
        let mut db = self.store.connect()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut job = Store::job(&tx, id)?;
        access(&tx, owner, &job)?;
        draft_agent(&tx, &job.plan.executor_id, Some(owner))?;
        if !["draft", "open"].contains(&job.state.as_str()) || job.plan.deadline <= Utc::now() {
            return Err(ApiError::conflict("This job is expired or closed."));
        }
        if pending(&tx, id)? {
            return Err(ApiError::conflict(
                "Confirm the pending wallet action before replacing evidence.",
            ));
        }
        let proof = json!({"output":evidence,"terms_hash":job.terms_hash});
        job.evidence_hash = Some(digest(&proof));
        job.evidence = Some(proof);
        Store::save_job(&tx, &job)?;
        tx.commit()?;
        Ok(job)
    }
    pub fn cancel_draft(&self, owner: &str, id: &str) -> Result<Job> {
        let mut db = self.store.connect()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut job = Store::job(&tx, id)?;
        access(&tx, owner, &job)?;
        let children:u64=tx.query_row("SELECT count(*) FROM jobs WHERE parent_id=? AND json_extract(payload,'$.state') NOT IN ('cancelled','accepted')",[id],|r|r.get(0))?;
        if job.funding != "unfunded" || job.state != "draft" || children > 0 {
            return Err(ApiError::conflict(
                "Close child branches first; funded jobs need a wallet cancellation.",
            ));
        }
        if !owns(&tx, owner, &job.requester_id)? && !owns(&tx, owner, &job.plan.executor_id)? {
            return Err(ApiError::forbidden(
                "Only the buyer or executor can cancel this draft.",
            ));
        }
        if pending(&tx, id)? {
            return Err(ApiError::conflict(
                "Confirm the pending wallet action first.",
            ));
        }
        if let Some(parent) = &job.parent_id {
            let mut parent = Store::job(&tx, parent)?;
            parent.available += job.plan.budget;
            Store::save_job(&tx, &parent)?;
        }
        job.state = "cancelled".into();
        job.available = Decimal::ZERO;
        if job.bond_status == "planned" {
            job.bond_status = "released".into();
            tx.execute(
                "UPDATE bond_reservations SET status='released' WHERE job_id=?",
                [id],
            )?;
        }
        Store::save_job(&tx, &job)?;
        tx.commit()?;
        Ok(job)
    }
    pub async fn job_system(&self) -> JobSystem {
        JobSystem {
            chain_id: 56,
            network: self.config.network.clone(),
            escrow: (!self.config.program.is_empty()).then(|| self.config.program.clone()),
            token: self.config.usdt.clone(),
            status: if self.bnb.deployed().await {
                "live"
            } else {
                "not_deployed"
            }
            .into(),
            max_depth: 8,
            review_window_seconds: 86400,
            service_payments_enabled: self.bnb.deployed().await
                && self.merchants().is_ok_and(|m| !m.is_empty()),
            error: None,
        }
    }
    pub fn job_instruction(&self, job: &Job, action: &str, _actor: &str) -> Result<Instruction> {
        let terms = Token::Tuple(vec![
            bnb::bytes32(&job.id)?,
            bnb::addr(&job.executor_wallet)?,
            bnb::uint(units(job.plan.budget)?),
            bnb::uint(units(job.plan.max_call)?),
            bnb::uint(job.plan.deadline.timestamp() as u128),
            bnb::bytes32(&job.terms_hash)?,
            bnb::uint(tool_bitmap(&job.plan.tools).into()),
            Token::Array(
                job.root_services
                    .iter()
                    .filter(|m| job.plan.services.contains(&m.id))
                    .map(|m| bnb::addr(&m.recipient))
                    .collect::<Result<Vec<_>>>()?,
            ),
        ]);
        let id = bnb::bytes32(&job.id)?;
        let (method, args) =
            match action {
                "fund" => ("openJob", vec![terms]),
                "delegate" => (
                    "delegateJob",
                    vec![
                        bnb::bytes32(job.parent_id.as_deref().ok_or_else(|| {
                            ApiError::validation("Only branches can be delegated.")
                        })?)?,
                        terms,
                    ],
                ),
                "pause" | "resume" => (
                    "pauseJob",
                    vec![bnb::bytes32(&job.root_id)?, Token::Bool(action == "pause")],
                ),
                "cancel" if job.parent_id.is_some() => ("returnBranch", vec![id]),
                "cancel" => ("cancelJob", vec![id]),
                "close_branch" => ("closeBranch", vec![id]),
                "submit" | "accept" => (
                    if action == "submit" {
                        "submitJob"
                    } else {
                        "acceptJob"
                    },
                    vec![
                        id,
                        bnb::bytes32(
                            job.evidence_hash
                                .as_deref()
                                .ok_or_else(|| ApiError::conflict("Attach evidence first."))?,
                        )?,
                    ],
                ),
                "reject" => ("rejectJob", vec![id]),
                _ => return Err(ApiError::validation("Choose a supported job action.")),
            };
        self.bnb.instruction("protocol", method, args)
    }
    pub async fn prepare_job(&self, owner: &str, id: &str, action: &str) -> Result<JobIntent> {
        self.bnb.require_deployment().await?;
        let (job, actor, existing) = {
            let mut connection = self.store.connect()?;
            let db =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let mut job = Store::job(&db, id)?;
            access(&db, owner, &job)?;
            let buyer_id = if let Some(parent) = &job.parent_id {
                Store::job(&db, parent)?.plan.executor_id
            } else {
                job.requester_id.clone()
            };
            let root_buyer_id = Store::job(&db, &job.root_id)?.requester_id;
            let root_cleanup = job.parent_id.is_some()
                && owns(&db, owner, &root_buyer_id)?
                && (action == "close_branch"
                    || (action == "cancel"
                        && Utc::now() > job.plan.deadline + Duration::seconds(86400)));
            let actor_id =
                if root_cleanup || ["accept", "reject", "pause", "resume"].contains(&action) {
                    &root_buyer_id
                } else if [
                    "fund",
                    "delegate",
                    "accept",
                    "reject",
                    "pause",
                    "resume",
                    "close_branch",
                ]
                .contains(&action)
                    || (action == "cancel" && owns(&db, owner, &buyer_id)?)
                {
                    &buyer_id
                } else {
                    &job.plan.executor_id
                };
            let allow_paused = [
                "submit",
                "accept",
                "reject",
                "cancel",
                "close_branch",
                "pause",
                "resume",
            ]
            .contains(&action);
            let actor = signing_agent(&db, actor_id, Some(owner), allow_paused)?;
            if ["fund", "delegate"].contains(&action)
                && job.funding == "unfunded"
                && (job.buyer_wallet.is_empty() || job.executor_wallet.is_empty())
            {
                let buyer = ready_agent(&db, &buyer_id, Some(owner))?;
                let (executor_owner, _) = Store::agent_any(&db, &job.plan.executor_id)?;
                let executor = ready_agent(
                    &db,
                    &job.plan.executor_id,
                    if executor_owner == owner {
                        Some(owner)
                    } else {
                        None
                    },
                )?;
                job.buyer_wallet = buyer.wallet.expect("ready buyer");
                job.executor_wallet = executor.wallet.expect("ready executor");
                job.terms = json!({"plan":job.plan,"id":job.id,"requester_id":job.requester_id,"buyer_wallet":job.buyer_wallet,"executor_wallet":job.executor_wallet,"parent_id":job.parent_id,"root_id":job.root_id,"root_services":job.root_services});
                job.terms_hash = digest(&job.terms);
                job.evidence = None;
                job.evidence_hash = None;
                Store::save_job(&db, &job)?;
            }
            let existing:Vec<JobIntent>=Store::list_payload(&db,"SELECT payload FROM job_intents WHERE job_id=? AND confirmed=0 AND (tx_hash IS NOT NULL OR julianday(json_extract(payload,'$.expires_at'))>julianday(?))",params![id,now()])?;
            for intent in &existing {
                if intent.action != action {
                    return Err(ApiError::conflict(
                        "Confirm the existing wallet action first.",
                    ));
                }
            }
            if action == "fund"
                && (job.parent_id.is_some() || job.funding != "unfunded" || job.state != "draft")
            {
                return Err(ApiError::conflict("Only an unfunded root can be funded."));
            }
            if action == "delegate"
                && (job.parent_id.is_none()
                    || Store::job(&db, job.parent_id.as_deref().expect("branch"))?.funding
                        != "funded")
            {
                return Err(ApiError::conflict("Fund the parent first."));
            }
            if !["fund", "delegate"].contains(&action) && job.funding != "funded" {
                return Err(ApiError::conflict("Fund the job first."));
            }
            if action == "submit"
                && (job.state != "open" || Utc::now().timestamp() > job.plan.deadline.timestamp())
            {
                return Err(ApiError::conflict(
                    "Evidence can only be submitted to an open job by its published deadline.",
                ));
            }
            db.commit()?;
            (job, actor, existing)
        };
        let wallet = actor.wallet.as_deref().expect("ready actor");
        let agent_address = actor.registry_id.as_deref().expect("registered actor");
        if actor.registry_address != self.config.program
            || self.bnb.agent_address(wallet, &actor.id)? != agent_address
        {
            return Err(ApiError::unavailable(
                "The signing agent does not match its exact registered BNB Smart Chain policy.",
            ));
        }
        let chain_agent = self.bnb.agent(agent_address).await?;
        if chain_agent["owner"] != wallet {
            return Err(ApiError::forbidden(
                "The signing wallet does not own this exact BNB agent.",
            ));
        }
        if chain_agent["paused"] == true
            && ![
                "submit",
                "accept",
                "reject",
                "cancel",
                "close_branch",
                "pause",
                "resume",
            ]
            .contains(&action)
        {
            return Err(ApiError::conflict("Resume the onchain agent policy first."));
        }
        if let Some(intent) = existing.first() {
            if intent.sender != wallet {
                return Err(ApiError::conflict(
                    "Confirm the existing wallet action with its original signing wallet first.",
                ));
            }
            return Ok(intent.clone());
        }
        let ix = self.job_instruction(&job, action, wallet)?;
        let approval = if action == "fund" {
            Some((self.config.usdt.as_str(), units(job.plan.budget)?))
        } else {
            None
        };
        let transactions = self.bnb.transactions(wallet, &ix, approval).await?;
        let intent = JobIntent {
            tx_hash: None,
            id: identifier(),
            job_id: id.into(),
            action: action.into(),
            chain_id: 56,
            network: "mainnet".into(),
            sender: wallet.into(),
            to: ix.to.clone(),
            data: ix.data.clone(),
            value: ix.value.clone(),
            approval: None,
            transactions,
            expires_at: (Utc::now() + Duration::minutes(10)).to_rfc3339(),
        };
        let mut db = self.store.connect()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if pending(&tx, id)? || crate::intents::has_pending_transaction(&tx, owner, &wallet)? {
            return Err(ApiError::conflict(
                "A wallet action was prepared concurrently. Refresh before signing.",
            ));
        }
        let current = Store::job(&tx, id)?;
        if encode(&current)? != encode(&job)? {
            return Err(ApiError::conflict(
                "Job terms or state changed while preparing the wallet action.",
            ));
        }
        tx.execute(
            "INSERT INTO job_intents(id,job_id,owner,payload,instruction) VALUES(?,?,?,?,?)",
            params![intent.id, id, owner, encode(&intent)?, encode(&ix)?],
        )?;
        tx.commit()?;
        Ok(intent)
    }
    pub async fn confirm_job(&self, owner: &str, id: &str, signature: &str) -> Result<Job> {
        self.bnb.require_deployment().await?;
        bnb::signature(signature)?;
        let row: Option<(String, Option<String>, u8, Option<String>)> = {
            self.store.connect()?.query_row("SELECT payload,tx_hash,confirmed,instruction FROM job_intents WHERE owner=? AND id=?",params![owner,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?
        };
        let (payload, existing, confirmed, instruction) =
            row.ok_or_else(|| ApiError::missing("Wallet action not found."))?;
        let intent: JobIntent = decode(payload)?;
        if existing.as_deref().is_some_and(|v| v != signature) {
            return Err(ApiError::conflict("Confirm the original transaction."));
        }
        if confirmed == 1 {
            return self.get_job(owner, &intent.job_id);
        }
        let job = self.get_job(owner, &intent.job_id)?;
        let ix = if let Some(instruction) = instruction {
            decode(instruction)?
        } else {
            self.job_instruction(&job, &intent.action, &intent.sender)?
        };
        self.bnb
            .verify_transaction(signature, &intent.sender, &ix)
            .await?;
        if self
            .store
            .connect()?
            .execute(
                "UPDATE job_intents SET tx_hash=? WHERE id=?",
                params![signature, id],
            )
            .is_err()
        {
            return Err(ApiError::conflict(
                "Transaction is already linked to another wallet action.",
            ));
        }
        self.store.claim_receipt(signature, &format!("job:{id}"))?;
        let job = self.refresh_job(owner, &job.id, Some(signature)).await?;
        if intent.action == "pay" {
            self.invoice_payment_event(owner, &intent, signature)?;
        }
        self.store
            .connect()?
            .execute("UPDATE job_intents SET confirmed=1 WHERE id=?", [id])?;
        Ok(job)
    }
    pub async fn refresh_job(&self, owner: &str, id: &str, signature: Option<&str>) -> Result<Job> {
        self.bnb.require_deployment().await?;
        let job = self.get_job(owner, id)?;
        let tree: Vec<Job> = {
            Store::list_payload(
                &self.store.connect()?,
                "SELECT payload FROM jobs WHERE root_id=?",
                [&job.root_id],
            )?
        };
        let mut updates = vec![];
        for mut node in tree.clone() {
            let state = self.bnb.job(&node.id).await?;
            if state["buyer"] == bnb::ZERO {
                continue;
            }
            if state["termsHash"] != node.terms_hash
                || state["buyer"] != node.buyer_wallet
                || state["executor"] != node.executor_wallet
                || bnb::number(&state["budget"])? != units(node.plan.budget)?
                || bnb::number(&state["maxCall"])? != units(node.plan.max_call)?
                || bnb::number(&state["deadline"])? != node.plan.deadline.timestamp() as u128
                || state["root"] != bnb::id(&node.root_id)?
                || bnb::number(&state["tools"])? != u128::from(tool_bitmap(&node.plan.tools))
                || bnb::number(&state["feeBps"])?
                    != self.bnb.manifest()?["fee_bps"].as_u64().unwrap_or(200) as u128
            {
                return Err(ApiError::unavailable(
                    "BNB escrow terms differ from the saved job.",
                ));
            }
            let recipients: Vec<_> = node
                .root_services
                .iter()
                .filter(|m| node.plan.services.contains(&m.id))
                .map(|m| bnb::address(&m.recipient))
                .collect::<Result<_>>()?;
            if state["recipients"] != json!(recipients) {
                return Err(ApiError::unavailable(
                    "Onchain provider recipients differ from the buyer-approved terms.",
                ));
            }
            let expected_parent = node
                .parent_id
                .as_deref()
                .map(bnb::id)
                .transpose()?
                .unwrap_or_else(|| bnb::ZERO_HASH.into());
            if state["parent"] != expected_parent {
                return Err(ApiError::unavailable(
                    "Onchain delegation parent differs from the saved terms.",
                ));
            }
            if state["agent"] != bnb::ZERO_HASH
                && state["agent"]
                    != self
                        .bnb
                        .agent_address(&node.executor_wallet, &node.plan.executor_id)?
            {
                return Err(ApiError::unavailable(
                    "Job is bound to a different registered agent.",
                ));
            }
            node.state = match bnb::number(&state["state"])? {
                0 => "draft",
                1 => "open",
                2 => "submitted",
                3 | 5 => "accepted",
                4 => "cancelled",
                _ => return Err(ApiError::unavailable("Invalid onchain job state.")),
            }
            .into();
            node.funding = "funded".into();
            node.available = money(bnb::number(&state["available"])?);
            node.reward_paid = money(bnb::number(&state["rewardPaid"])?);
            node.refunded = money(bnb::number(&state["refunded"])?);
            node.fee_paid = money(bnb::number(&state["feePaid"])?);
            node.provider_paid = money(bnb::number(&state["providerSpent"])?);
            node.timely_submitted = state["timelySubmitted"] == true;
            node.submitted_at = (bnb::number(&state["submittedAt"])? as i64 > 0)
                .then_some(bnb::number(&state["submittedAt"])? as i64);
            node.cancelled_at = (bnb::number(&state["cancelledAt"])? as i64 > 0)
                .then_some(bnb::number(&state["cancelledAt"])? as i64);
            let evidence = state["evidence"]
                .as_str()
                .ok_or_else(|| ApiError::unavailable("Invalid job evidence commitment."))?;
            if evidence != bnb::ZERO_HASH {
                if node.evidence_hash.as_deref() != Some(evidence) {
                    node.evidence = None;
                }
                node.evidence_hash = Some(evidence.into());
            }
            if node.id == id {
                if let Some(signature) = signature {
                    node.chain_tx = Some(signature.into());
                }
            }
            node.paused = state["paused"] == true;
            updates.push(node);
        }
        let paused = updates
            .iter()
            .find(|j| j.id == job.root_id)
            .is_some_and(|j| j.paused);
        let mut db = self.store.connect()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let fresh_tree: Vec<Job> = Store::list_payload(
            &tx,
            "SELECT payload FROM jobs WHERE root_id=?",
            [&job.root_id],
        )?;
        for mut node in updates {
            let reserved = fresh_tree
                .iter()
                .filter(|j| {
                    j.parent_id.as_deref() == Some(&node.id)
                        && j.state == "draft"
                        && j.funding == "unfunded"
                        && !node.id.eq(&j.id)
                })
                .map(|j| j.plan.budget)
                .sum::<Decimal>();
            node.available = (node.available - reserved).max(Decimal::ZERO);
            node.paused = paused;
            let original = Store::job(&tx, &node.id)?;
            if original.terms_hash != node.terms_hash {
                return Err(ApiError::conflict(
                    "Saved terms changed while reading BNB Smart Chain state.",
                ));
            }
            // Preserve offchain evidence that was added after the snapshot.
            if original.evidence_hash == node.evidence_hash {
                node.evidence = original.evidence;
            }
            Store::save_job(&tx, &node)?;
        }
        tx.commit()?;
        self.get_job(owner, id)
    }
}
