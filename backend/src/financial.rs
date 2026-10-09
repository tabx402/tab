use crate::{
    bnb::{self, Instruction},
    db::{decode, encode, Store},
    error::{ApiError, Result},
    models::*,
    AppState,
};
use chrono::{Duration, Utc};
use ethabi::Token;
use rusqlite::{params, OptionalExtension};
use rust_decimal::Decimal;
use serde_json::{json, Value};
fn positive(amount: Decimal) -> Result<u128> {
    if amount <= Decimal::ZERO {
        return Err(ApiError::validation("Use a positive token amount."));
    }
    units(amount)
}
fn required<'a>(value: &'a Option<String>, message: &str) -> Result<&'a str> {
    value
        .as_deref()
        .ok_or_else(|| ApiError::validation(message))
}
impl AppState {
    async fn require_tab(&self) -> Result<String> {
        if self.token_system().await["status"] != "verified" {
            return Err(ApiError::unavailable(
                "The official TAB token contract is not verified.",
            ));
        }
        Ok(self.config.official_tab.clone().expect("verified TAB"))
    }
    pub(crate) async fn verify_owned_agent(&self, agent: &RuntimeAgent) -> Result<String> {
        let wallet = agent
            .wallet
            .as_deref()
            .ok_or_else(|| ApiError::conflict("Register the agent first."))?;
        if agent.registry_address != self.config.program
            || agent.registry_id.as_deref()
                != Some(self.bnb.agent_address(wallet, &agent.id)?.as_str())
        {
            return Err(ApiError::unavailable(
                "Agent is not registered in this exact BNB deployment.",
            ));
        }
        let chain = self
            .bnb
            .agent(agent.registry_id.as_deref().expect("registered agent"))
            .await?;
        if chain["owner"] != wallet {
            return Err(ApiError::forbidden(
                "The onchain agent owner differs from the verified wallet.",
            ));
        }
        Ok(wallet.into())
    }
    pub async fn financial_prepare(
        &self,
        owner: &str,
        id: &str,
        mut input: FinancialInput,
    ) -> Result<TransactionIntent> {
        if ["open_credit", "accept_credit", "spend_credit", "pledge_collateral", "withdraw_collateral", "liquidate_credit"].contains(&input.action.as_str()) && self.bnb.manifest()?["credit_mode"] != "collateralized" {
            return Err(ApiError::unavailable("Secured credit requires the current collateralized deployment."));
        }
        self.bnb.require_deployment().await?;
        let agent = self.store.agent(owner, id)?;
        let recovery = [
            "withdraw_bnb",
            "withdraw_backing",
            "withdraw_spending",
            "unstake",
            "settle_bond",
            "resolve_outcome",
            "claim_outcome",
            "revoke_session",
            "pause_agent",
            "repay_credit",
            "withdraw_credit",
            "close_credit",
            "pledge_collateral",
            "withdraw_collateral",
            "liquidate_credit",
        ]
        .contains(&input.action.as_str());
        if agent.status != "ready" && !(agent.status == "paused" && recovery) {
            return Err(ApiError::conflict("Register or resume the agent first."));
        }
        let wallet = self.verify_owned_agent(&agent).await?;
        let agent_key = agent.registry_id.as_deref().expect("registered agent");
        let chain = self.bnb.agent(agent_key).await?;
        if chain["paused"] == true && !recovery {
            return Err(ApiError::conflict(
                "Resume the onchain agent before a new commitment.",
            ));
        }
        if ["back", "withdraw_backing"].contains(&input.action.as_str())
            && input.token_address.is_none()
        {
            input.token_address = Some(self.config.usdt.clone());
        }
        if let Some(token) = &mut input.token_address {
            *token = bnb::address(token)?;
        }
        let pending:Vec<TransactionIntent>=Store::list_payload(&self.store.connect()?,"SELECT payload FROM wallet_intents WHERE owner=? AND agent_id=? AND confirmed=0 AND (tx_hash IS NOT NULL OR julianday(json_extract(payload,'$.expires_at'))>julianday(?))",params![owner,id,now()])?;
        for intent in pending {
            if intent.action == input.action && intent.details == serde_json::to_value(&input)? {
                return Ok(intent);
            }
            return Err(ApiError::conflict(
                "Confirm the existing wallet action before preparing another.",
            ));
        }
        if crate::intents::has_pending_transaction(&self.store.connect()?, owner, &wallet)? {
            return Err(ApiError::conflict(
                "Resolve the pending transaction for this wallet before preparing another.",
            ));
        }
        let mut approval: Option<(String, u128)> = None;
        let mut value = "0x0".to_string();
        let target = if let Some(target) = &input.target_agent_id {
            let (actual_owner, a) = Store::agent_any(&self.store.connect()?, target)?;
            if actual_owner != owner && !a.plan.public_activity {
                return Err(ApiError::missing("Public backing target not found."));
            }
            self.verify_owned_agent(&a).await?;
            a.registry_id.expect("registered backing target")
        } else {
            agent_key.to_owned()
        };
        if [
            "accept_credit",
            "spend_credit",
            "repay_credit",
            "withdraw_credit",
            "close_credit",
            "pledge_collateral",
            "withdraw_collateral",
            "liquidate_credit",
        ]
        .contains(&input.action.as_str())
        {
            let credit = required(&input.credit_id, "Choose the exact credit agreement.")?;
            let c = self
                .bnb
                .view("backing", "getCredit", vec![bnb::bytes32(credit)?])
                .await?;
            if c["lender"] == bnb::ZERO {
                return Err(ApiError::missing("Credit agreement not found."));
            }
            let borrower = c["borrower"] == wallet && c["agent"] == agent_key;
            let lender = c["lender"] == wallet;
            if ([
                "accept_credit",
                "spend_credit",
                "repay_credit",
                "pledge_collateral",
                "withdraw_collateral",
            ]
            .contains(&input.action.as_str())
                && !borrower)
                || (input.action == "withdraw_credit" && !lender)
                || (input.action == "close_credit" && !borrower && !lender)
            {
                return Err(ApiError::forbidden(
                    "This credit action belongs to a different agent or wallet.",
                ));
            }
        }
        let ix = match input.action.as_str() {
            "stake" | "unstake" => {
                let token = self.require_tab().await?;
                if input.token_address.as_ref().is_some_and(|t| *t != token) {
                    return Err(ApiError::validation(
                        "Staking uses the official TAB token only.",
                    ));
                }
                if input.action == "stake" {
                    let decimals = self.bnb.token_decimals(&token).await?;
                    let amount = token_units(input.amount, decimals)?;
                    if amount == 0 {
                        return Err(ApiError::validation("Choose a positive stake amount."));
                    }
                    let lock = input.lock_seconds.unwrap_or(86400);
                    if !(86400..=31 * 86400).contains(&lock) {
                        return Err(ApiError::validation(
                            "Stake locks last from one to thirty-one days.",
                        ));
                    }
                    approval = Some((token, amount));
                    self.bnb.instruction(
                        "economics",
                        "stakeTab",
                        vec![bnb::uint(amount), bnb::uint(lock as u128)],
                    )?
                } else {
                    self.bnb.instruction("economics", "unstakeTab", vec![])?
                }
            }
            "pair_token" | "deploy_token" => {
                let commitment = bnb::bytes32(required(
                    &input.metadata_hash,
                    "Publish a metadata commitment hash.",
                )?)?;
                if input.metadata_hash.as_deref() == Some(bnb::ZERO_HASH) {
                    return Err(ApiError::validation(
                        "The metadata commitment cannot be zero.",
                    ));
                }
                if input.action == "pair_token" {
                    let token = required(&input.token_address, "Choose an agent token contract.")?;
                    self.bnb.token_decimals(token).await?;
                    self.bnb.instruction(
                        "economics",
                        "pairAgentToken",
                        vec![bnb::bytes32(agent_key)?, bnb::addr(token)?, commitment],
                    )?
                } else {
                    let name = clean(required(&input.name, "Choose a token name.")?, 2, 48)?;
                    let symbol = clean(required(&input.symbol, "Choose a token symbol.")?, 1, 10)?;
                    self.bnb.instruction(
                        "economics",
                        "deployAgentToken",
                        vec![
                            bnb::bytes32(agent_key)?,
                            Token::String(name),
                            Token::String(symbol),
                            bnb::uint(positive(input.amount)?),
                            commitment,
                        ],
                    )?
                }
            }
            "back" | "withdraw_backing" => {
                let token = input.token_address.as_deref().unwrap_or(&self.config.usdt);
                let allowed = self.backing_assets().as_array().is_some_and(|a| {
                    a.iter().any(|v| {
                        v["enabled"] == true
                            && v["address"]
                                .as_str()
                                .is_some_and(|v| v.eq_ignore_ascii_case(token))
                    })
                });
                if !allowed {
                    return Err(ApiError::validation(
                        "Choose a configured backing token on BNB mainnet.",
                    ));
                }
                let asset = self
                    .backing_assets()
                    .as_array()
                    .and_then(|a| a.iter().find(|a| a["address"] == token))
                    .cloned()
                    .ok_or_else(|| ApiError::validation("Backing asset is unavailable."))?;
                self.bnb.verify_backing_asset(&asset).await?;
                let decimals = self.bnb.token_decimals(token).await?;
                let amount = if input.action == "withdraw_backing" && input.amount == Decimal::ZERO
                {
                    bnb::number(
                        &self
                            .bnb
                            .view(
                                "backing",
                                "getBacking",
                                vec![
                                    bnb::bytes32(&target)?,
                                    bnb::addr(&wallet)?,
                                    bnb::addr(token)?,
                                ],
                            )
                            .await?["amount"],
                    )?
                } else {
                    let a = token_units(input.amount, decimals)?;
                    if a == 0 {
                        return Err(ApiError::validation("Choose a positive backing amount."));
                    }
                    a
                };
                if input.action == "back" {
                    approval = Some((token.into(), amount));
                }
                self.bnb.instruction(
                    "backing",
                    if input.action == "back" {
                        "backAgent"
                    } else {
                        "withdrawBacking"
                    },
                    vec![bnb::bytes32(&target)?, bnb::addr(token)?, bnb::uint(amount)],
                )?
            }
            "back_bnb" | "withdraw_bnb" => {
                if input.action == "back_bnb" {
                    value = format!("0x{:x}", positive(input.amount)?);
                    self.bnb
                        .instruction("backing", "backBNB", vec![bnb::bytes32(&target)?])?
                } else {
                    let amount = if input.amount == Decimal::ZERO {
                        bnb::number(
                            &self
                                .bnb
                                .view(
                                    "backing",
                                    "getBacking",
                                    vec![
                                        bnb::bytes32(&target)?,
                                        bnb::addr(&wallet)?,
                                        bnb::addr(bnb::ZERO)?,
                                    ],
                                )
                                .await?["amount"],
                        )?
                    } else {
                        positive(input.amount)?
                    };
                    self.bnb.instruction(
                        "backing",
                        "withdrawBNB",
                        vec![bnb::bytes32(&target)?, bnb::uint(amount)],
                    )?
                }
            }
            "initialize_spending" | "fund_spending" | "withdraw_spending" => {
                let amount = positive(input.amount)?;
                if input.action != "withdraw_spending" {
                    approval = Some((self.config.usdt.clone(), amount));
                }
                self.bnb.instruction(
                    "protocol",
                    if input.action == "withdraw_spending" {
                        "withdrawSpending"
                    } else {
                        "fundSpending"
                    },
                    vec![bnb::bytes32(agent_key)?, bnb::uint(amount)],
                )?
            }
            "bond_job" | "settle_bond" | "bind_job_agent" => {
                let job =
                    self.get_job(owner, required(&input.job_id, "Choose a job commitment.")?)?;
                if job.plan.executor_id != id
                    || job.executor_wallet != wallet
                    || job.funding != "funded"
                {
                    return Err(ApiError::forbidden(
                        "Only this funded job's exact assigned agent can prepare this action.",
                    ));
                }
                if input.action == "bind_job_agent" {
                    self.bnb.instruction(
                        "protocol",
                        "bindJobAgent",
                        vec![bnb::bytes32(&job.id)?, bnb::bytes32(agent_key)?],
                    )?
                } else {
                    let token = required(
                        &job.plan.bond_token_address,
                        "This commitment has no token bond.",
                    )?;
                    let paired = self
                        .bnb
                        .view("economics", "getLaunch", vec![bnb::bytes32(agent_key)?])
                        .await?;
                    if paired["token"] != token {
                        return Err(ApiError::validation(
                            "The bond must use this agent's exact paired token.",
                        ));
                    }
                    if input.action == "bond_job" {
                        if job.state != "open"
                            || input.amount != job.plan.bond_tokens
                            || input.penalty_bps.is_some_and(|p| p != job.plan.penalty_bps)
                        {
                            return Err(ApiError::validation(
                                "Use the published open job's exact bond amount and penalty.",
                            ));
                        }
                        let decimals = self.bnb.token_decimals(token).await?;
                        let amount = token_units(job.plan.bond_tokens, decimals)?;
                        approval = Some((token.into(), amount));
                        self.bnb.instruction(
                            "economics",
                            "bondJob",
                            vec![
                                bnb::bytes32(&job.id)?,
                                bnb::uint(amount),
                                bnb::uint(job.plan.penalty_bps.into()),
                            ],
                        )?
                    } else {
                        self.bnb.instruction(
                            "economics",
                            "settleBond",
                            vec![bnb::bytes32(&job.id)?],
                        )?
                    }
                }
            }
            "fund_bounty" => {
                let bounty_id = required(&input.bounty_id, "Choose a bounty draft.")?.to_owned();
                let raw: Option<String> = self
                    .store
                    .connect()?
                    .query_row(
                        "SELECT payload FROM bounties WHERE owner=? AND id=?",
                        params![owner, bounty_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                let mut bounty: Bounty =
                    decode(raw.ok_or_else(|| ApiError::missing("Bounty draft not found."))?)?;
                if bounty.funding != "unfunded" || bounty.assigned_agent_id.is_some() {
                    return Err(ApiError::conflict("Bounty is already funded or assigned."));
                }
                if bounty.plan.deadline <= Utc::now()
                    || bounty.plan.budget > agent.plan.daily_cap
                    || bounty
                        .plan
                        .tools
                        .iter()
                        .any(|t| !agent.plan.tools.contains(t))
                {
                    return Err(ApiError::validation(
                        "Bounty must fit this buyer agent's policy.",
                    ));
                }
                let jid = format!("{bounty_id}{}", "00".repeat(16));
                input.job_id = Some(jid.clone());
                let plan = JobInput {
                    title: bounty.plan.title.clone(),
                    description: bounty.plan.description.clone(),
                    executor_id: "00".repeat(16),
                    budget: bounty.plan.budget,
                    max_call: agent.plan.max_call.min(bounty.plan.budget),
                    deadline: bounty.plan.deadline,
                    tools: bounty.plan.tools.clone(),
                    acceptance: "buyer_review".into(),
                    minimum_block: 1,
                    public_activity: bounty.plan.public_activity,
                    services: vec![],
                    bond_tokens: Decimal::ZERO,
                    bond_token_address: None,
                    penalty_rule: None,
                    penalty_bps: 10000,
                };
                let terms =
                    json!({"bounty_id":bounty_id,"job_id":jid,"buyer_wallet":wallet,"plan":plan});
                let job = Job {
                    plan,
                    id: jid.clone(),
                    root_id: jid.clone(),
                    parent_id: None,
                    requester_id: id.into(),
                    buyer_wallet: wallet.clone(),
                    executor_wallet: bnb::ZERO.into(),
                    executor_name: "unassigned".into(),
                    created_at: now(),
                    depth: 0,
                    terms_hash: digest(&terms),
                    state: "draft".into(),
                    funding: "unfunded".into(),
                    available: bounty.plan.budget,
                    evidence_hash: None,
                    evidence: None,
                    paused: false,
                    chain_tx: None,
                    provider_paid: Decimal::ZERO,
                    reward_paid: Decimal::ZERO,
                    refunded: Decimal::ZERO,
                    root_services: vec![],
                    bond_status: "none".into(),
                    fee_paid: Decimal::ZERO,
                    timely_submitted: false,
                    submitted_at: None,
                    cancelled_at: None,
                    terms,
                };
                let mut db = self.store.connect()?;
                let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                if let Some(existing) = &bounty.job_id {
                    let retained = Store::job(&tx, existing)?;
                    if existing != &jid || retained.terms_hash != job.terms_hash {
                        return Err(ApiError::conflict(
                            "Bounty funding is bound to its original policy.",
                        ));
                    }
                } else {
                    tx.execute(
                        "INSERT INTO jobs VALUES(?,?,?,?,?,?)",
                        params![
                            jid,
                            owner,
                            encode(&job)?,
                            jid,
                            Option::<String>::None,
                            job.created_at
                        ],
                    )?;
                    bounty.job_id = Some(jid);
                    tx.execute(
                        "UPDATE bounties SET payload=? WHERE id=?",
                        params![encode(&bounty)?, bounty_id],
                    )?;
                }
                tx.commit()?;
                approval = Some((self.config.usdt.clone(), units(job.plan.budget)?));
                self.job_instruction(&job, "fund", &wallet)?
            }
            "claim_bounty" => {
                let bounty_id = required(&input.bounty_id, "Choose a public bounty.")?;
                let raw:Option<String>=self.store.connect()?.query_row("SELECT payload FROM bounties WHERE id=? AND json_extract(payload,'$.public_activity')=1",[bounty_id],|r|r.get(0)).optional()?;
                let bounty: Bounty =
                    decode(raw.ok_or_else(|| ApiError::missing("Public bounty not found."))?)?;
                if bounty.funding != "funded" || bounty.assigned_agent_id.is_some() {
                    return Err(ApiError::conflict("Choose a funded, unassigned bounty."));
                }
                if self.eligibility(owner, id).await?["eligible"] != true {
                    return Err(ApiError::forbidden(
                        "Hold official TAB and the agent's paired token before claiming.",
                    ));
                }
                let jid = required(&bounty.job_id, "Bounty escrow is unavailable.")?.to_owned();
                let job = Store::job(&self.store.connect()?, &jid)?;
                if job.executor_wallet != bnb::ZERO
                    || job.state != "open"
                    || job.plan.deadline <= Utc::now()
                    || job.plan.max_call > agent.plan.max_call
                    || job.plan.tools.iter().any(|t| !agent.plan.tools.contains(t))
                {
                    return Err(ApiError::conflict(
                        "This bounty cannot be claimed by the selected agent.",
                    ));
                }
                input.job_id = Some(jid.clone());
                self.bnb.instruction(
                    "protocol",
                    "claimJob",
                    vec![bnb::bytes32(&jid)?, bnb::bytes32(agent_key)?],
                )?
            }
            "open_outcome" | "bet_outcome" | "resolve_outcome" | "claim_outcome" => {
                self.require_tab().await?;
                let jid = required(&input.job_id, "Choose a funded commitment.")?;
                let job = match self.get_job(owner, jid) {
                    Ok(j) => j,
                    Err(_) => {
                        if !self.public_jobs()?.iter().any(|j| j.id == jid) {
                            return Err(ApiError::missing("Public job not found."));
                        }
                        Store::job(&self.store.connect()?, jid)?
                    }
                };
                if job.funding != "funded" {
                    return Err(ApiError::conflict("Fund the commitment first."));
                }
                let (method, args) = match input.action.as_str() {
                    "open_outcome" => {
                        if job.plan.executor_id != id || job.executor_wallet != wallet {
                            return Err(ApiError::forbidden(
                                "Only the exact assigned agent can open this outcome pool.",
                            ));
                        }
                        let closes = input
                            .closes_at
                            .ok_or_else(|| ApiError::validation("Choose a closing time."))?;
                        if closes <= Utc::now().timestamp()
                            || closes > job.plan.deadline.timestamp()
                        {
                            return Err(ApiError::validation(
                                "Outcome positions close before the job deadline.",
                            ));
                        }
                        (
                            "openOutcome",
                            vec![bnb::bytes32(jid)?, bnb::uint(closes as u128)],
                        )
                    }
                    "bet_outcome" => {
                        let amount = positive(input.amount)?;
                        approval = Some((self.config.usdt.clone(), amount));
                        (
                            "betOutcome",
                            vec![
                                bnb::bytes32(jid)?,
                                bnb::uint(amount),
                                Token::Bool(input.side.ok_or_else(|| {
                                    ApiError::validation("Choose for or against timely submission.")
                                })?),
                            ],
                        )
                    }
                    "resolve_outcome" => ("resolveOutcome", vec![bnb::bytes32(jid)?]),
                    _ => ("claimOutcome", vec![bnb::bytes32(jid)?]),
                };
                self.bnb.instruction("economics", method, args)?
            }
            "grant_session" | "revoke_session" => {
                let nonce = input.session_nonce.unwrap_or_else(rand::random);
                if input.action == "revoke_session" && input.session_nonce.is_none() {
                    return Err(ApiError::validation(
                        "Choose the exact session nonce to revoke.",
                    ));
                }
                input.session_nonce = Some(nonce);
                if input.action == "revoke_session" {
                    let sid = self
                        .bnb
                        .view(
                            "protocol",
                            "sessionId",
                            vec![bnb::bytes32(agent_key)?, bnb::uint(nonce.into())],
                        )
                        .await?;
                    self.bnb.instruction(
                        "protocol",
                        "revokeSession",
                        vec![bnb::bytes32(sid.as_str().ok_or_else(ApiError::internal)?)?],
                    )?
                } else {
                    let (per, daily, total, expires, recipients) =
                        self.session_terms(&agent, &input)?;
                    let signer = bnb::addr(required(
                        &input.session_signer,
                        "Choose a delegated signer address.",
                    )?)?;
                    self.bnb.instruction(
                        "protocol",
                        "grantSession",
                        vec![
                            bnb::bytes32(agent_key)?,
                            Token::Tuple(vec![
                                bnb::uint(nonce.into()),
                                signer,
                                bnb::uint(expires as u128),
                                bnb::uint(per),
                                bnb::uint(daily),
                                bnb::uint(total),
                                bnb::uint(tool_bitmap(&input.tools).into()),
                                Token::Array(recipients),
                            ]),
                        ],
                    )?
                }
            }
            "pause_agent" => self.bnb.instruction(
                "protocol",
                "pauseAgent",
                vec![
                    bnb::bytes32(agent_key)?,
                    Token::Bool(
                        input
                            .paused
                            .ok_or_else(|| ApiError::validation("Choose pause or resume."))?,
                    ),
                ],
            )?,
            "open_credit" => {
                let credit = input
                    .credit_id
                    .clone()
                    .unwrap_or_else(|| format!("{}{}", identifier(), identifier()));
                input.credit_id = Some(credit.clone());
                let (per, daily, total, expires, recipients) =
                    self.session_terms(&agent, &input)?;
                let signer = bnb::addr(required(
                    &input.session_signer,
                    "Choose the credit signer.",
                )?)?;
                approval = Some((self.config.usdt.clone(), total));
                let collateral = input.token_address.clone().unwrap_or_else(|| self.config.usdt.clone());
                let risk = self.bnb.view("backing", "collateralAssets", vec![bnb::addr(&collateral)?]).await?;
                if bnb::number(&risk["ltvBps"])? == 0 || risk["paused"] == true {
                    return Err(ApiError::unavailable("This collateral does not have an enabled token price oracle and borrowing policy."));
                }
                input.token_address = Some(collateral.clone());
                self.bnb.instruction(
                    "backing",
                    "openCreditWithCollateral",
                    vec![Token::Tuple(vec![
                        bnb::bytes32(&credit)?,
                        bnb::bytes32(&target)?,
                        signer,
                        bnb::uint(total),
                        bnb::uint(per),
                        bnb::uint(daily),
                        bnb::uint(expires as u128),
                        bnb::uint(tool_bitmap(&input.tools).into()),
                        Token::Array(recipients),
                    ]), bnb::addr(&collateral)?],
                )?
            }
            "pledge_collateral" | "withdraw_collateral" | "liquidate_credit" => {
                let credit = required(&input.credit_id, "Choose the exact credit agreement.")?;
                let position = self.bnb.view("backing", "collateralPositions", vec![bnb::bytes32(credit)?]).await?;
                let token = position["token"].as_str().ok_or_else(ApiError::internal)?;
                let decimals = self.bnb.token_decimals(token).await?;
                if input.token_address.as_deref().is_some_and(|t|t!=token) {
                    return Err(ApiError::validation("Use the collateral token named in this credit agreement."));
                }
                input.token_address = Some(token.into());
                let mut args = vec![bnb::bytes32(credit)?];
                let method = if input.action == "liquidate_credit" {
                    let amount = positive(input.amount)?;
                    approval = Some((self.config.usdt.clone(), amount));
                    let minimum = input.minimum_collateral_out.ok_or_else(||ApiError::validation("Set the minimum collateral you will receive."))?;
                    let minimum = token_units(minimum, decimals)?;
                    if minimum == 0 { return Err(ApiError::validation("The minimum collateral must be positive.")); }
                    args.extend([bnb::uint(amount),bnb::uint(minimum)]);
                    "liquidateCredit"
                } else {
                    let amount = token_units(input.amount, decimals)?;
                    if amount == 0 { return Err(ApiError::validation("Choose a positive collateral amount.")); }
                    args.push(bnb::uint(amount));
                    if input.action == "pledge_collateral" {
                        approval = Some((token.into(), amount));
                        "pledgeCollateral"
                    } else { "withdrawCollateral" }
                };
                self.bnb.instruction("backing",method,args)?
            }
            "spend_credit" | "session_pay" => {
                let recipient = bnb::address(required(
                    &input.recipient,
                    "Choose the approved service recipient.",
                )?)?;
                let tool = required(&input.tool, "Choose the exact service tool.")?;
                if !agent.plan.tools.iter().any(|t| t == tool)
                    || !self
                        .x402_merchants()?
                        .iter()
                        .any(|m| m.recipient.eq_ignore_ascii_case(&recipient))
                {
                    return Err(ApiError::validation(
                        "Payment must use an approved tool and merchant recipient.",
                    ));
                }
                let request =
                    required(&input.request_hash, "Bind the exact service request hash.")?;
                let receipt =
                    required(&input.receipt_hash, "Bind the provider receipt commitment.")?;
                if bnb::id(request)? == bnb::ZERO_HASH || bnb::id(receipt)? == bnb::ZERO_HASH {
                    return Err(ApiError::validation("Payment commitments cannot be zero."));
                }
                let payment_id = if input.action == "spend_credit" {
                    bnb::id(required(
                        &input.credit_id,
                        "Choose the exact credit agreement.",
                    )?)?
                } else {
                    let nonce = input
                        .session_nonce
                        .ok_or_else(|| ApiError::validation("Choose the exact session nonce."))?;
                    self.bnb
                        .view(
                            "protocol",
                            "sessionId",
                            vec![bnb::bytes32(agent_key)?, bnb::uint(nonce.into())],
                        )
                        .await?
                        .as_str()
                        .ok_or_else(ApiError::internal)?
                        .to_owned()
                };
                self.bnb.instruction(
                    if input.action == "spend_credit" {
                        "backing"
                    } else {
                        "protocol"
                    },
                    if input.action == "spend_credit" {
                        "spendCredit"
                    } else {
                        "sessionPay"
                    },
                    vec![
                        bnb::bytes32(&payment_id)?,
                        bnb::addr(&recipient)?,
                        bnb::uint(positive(input.amount)?),
                        bnb::uint(tool_bitmap(&[tool.into()]).into()),
                        bnb::bytes32(request)?,
                        bnb::bytes32(receipt)?,
                    ],
                )?
            }
            "accept_credit" | "close_credit" | "repay_credit" | "withdraw_credit" => {
                let credit = required(&input.credit_id, "Choose the exact credit agreement.")?;
                let mut args = vec![bnb::bytes32(credit)?];
                let method = match input.action.as_str() {
                    "accept_credit" => "acceptCredit",
                    "close_credit" => "closeCredit",
                    "repay_credit" => {
                        let amount = positive(input.amount)?;
                        approval = Some((self.config.usdt.clone(), amount));
                        args.push(bnb::uint(amount));
                        "repayCredit"
                    }
                    _ => {
                        args.push(bnb::uint(positive(input.amount)?));
                        "withdrawCredit"
                    }
                };
                self.bnb.instruction("backing", method, args)?
            }
            _ => return Err(ApiError::validation("Choose a supported wallet action.")),
        };
        let ix = Instruction { value, ..ix };
        let transactions = self
            .bnb
            .transactions(
                &wallet,
                &ix,
                approval.as_ref().map(|(t, a)| (t.as_str(), *a)),
            )
            .await?;
        let details = serde_json::to_value(&input)?;
        let intent = TransactionIntent {
            tx_hash: None,
            id: identifier(),
            agent_id: id.into(),
            action: input.action,
            chain_id: 56,
            network: "mainnet".into(),
            sender: wallet,
            to: ix.to.clone(),
            data: ix.data.clone(),
            value: ix.value.clone(),
            transaction: ix.transaction(),
            transactions,
            expires_at: (Utc::now() + Duration::minutes(10)).to_rfc3339(),
            details,
        };
        let mut db = self.store.connect()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let pending = crate::intents::has_pending_transaction(&tx, owner, &intent.sender)?;
        if pending {
            return Err(ApiError::conflict(
                "Another wallet action was prepared concurrently. Refresh before signing.",
            ));
        }
        tx.execute(
            "INSERT INTO wallet_intents(id,owner,agent_id,payload,instruction) VALUES(?,?,?,?,?)",
            params![intent.id, owner, id, encode(&intent)?, encode(&ix)?],
        )?;
        tx.commit()?;
        Ok(intent)
    }
    fn session_terms(
        &self,
        agent: &RuntimeAgent,
        input: &FinancialInput,
    ) -> Result<(u128, u128, u128, i64, Vec<Token>)> {
        let expires = input
            .session_expires_at
            .ok_or_else(|| ApiError::validation("Set the authorization expiry."))?;
        let per = input.per_call.unwrap_or(agent.plan.max_call);
        let daily = input.daily_cap.unwrap_or(agent.plan.daily_cap);
        if expires <= Utc::now().timestamp()
            || expires > Utc::now().timestamp() + 7 * 86400
            || per <= Decimal::ZERO
            || daily <= Decimal::ZERO
            || per > agent.plan.max_call
            || daily > agent.plan.daily_cap
            || per > daily
            || input.amount <= Decimal::ZERO
            || daily > input.amount
            || input.amount > daily * Decimal::from(7)
        {
            return Err(ApiError::validation(
                "Keep authorization limits and seven-day expiry within the agent policy.",
            ));
        }
        if input.tools.is_empty()
            || !distinct(&input.tools)
            || input.tools.iter().any(|t| !agent.plan.tools.contains(t))
        {
            return Err(ApiError::validation(
                "Choose a subset of the agent's approved tools.",
            ));
        }
        let merchants = self.x402_merchants()?;
        let allowed: std::collections::HashSet<_> = merchants
            .iter()
            .map(|m| m.recipient.to_lowercase())
            .collect();
        if input.recipients.is_empty()
            || input.recipients.len() > 8
            || !distinct(&input.recipients)
            || input
                .recipients
                .iter()
                .any(|r| !allowed.contains(&r.to_lowercase()))
        {
            return Err(ApiError::validation(
                "Choose distinct operator-approved merchant recipients.",
            ));
        }
        Ok((
            units(per)?,
            units(daily)?,
            units(input.amount)?,
            expires,
            input
                .recipients
                .iter()
                .map(|r| bnb::addr(r))
                .collect::<Result<Vec<_>>>()?,
        ))
    }
    pub async fn financial_confirm(&self, owner: &str, id: &str, signature: &str) -> Result<Value> {
        self.bnb.require_deployment().await?;
        bnb::signature(signature)?;
        let signature = signature.to_lowercase();
        let row:Option<(String,String,Option<String>,u8)>=self.store.connect()?.query_row("SELECT payload,instruction,tx_hash,confirmed FROM wallet_intents WHERE owner=? AND id=?",params![owner,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let (payload, instruction, previous, confirmed) =
            row.ok_or_else(|| ApiError::missing("Wallet action not found."))?;
        let intent: TransactionIntent = decode(payload)?;
        let ix: Instruction = decode(instruction)?;
        if previous.as_ref().is_some_and(|v| v != &signature) {
            return Err(ApiError::conflict(
                "Confirm the originally submitted transaction.",
            ));
        }
        if confirmed == 1 {
            return Ok(json!({"status":"confirmed","tx_hash":signature,"intent":intent}));
        }
        if intent.details["finance_module"].as_str().is_some() {
            self.finance_verify_intent(&intent).await?;
        }
        if ["back", "withdraw_backing"].contains(&intent.action.as_str()) {
            let assets = self.backing_assets();
            let asset = assets
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .find(|a| a["address"] == intent.details["token_address"])
                })
                .ok_or_else(|| ApiError::unavailable("Backing token configuration changed."))?;
            self.bnb.verify_backing_asset(asset).await?;
        }
        let receipt = self
            .bnb
            .verify_transaction(&signature, &intent.sender, &ix)
            .await?;
        self.store
            .claim_receipt(&signature, &format!("wallet:{id}"))?;
        if ["fund_bounty", "claim_bounty"].contains(&intent.action.as_str()) {
            let bid = intent.details["bounty_id"]
                .as_str()
                .ok_or_else(ApiError::internal)?;
            let jid = intent.details["job_id"]
                .as_str()
                .ok_or_else(ApiError::internal)?;
            if intent.action == "claim_bounty" {
                let chain = self.bnb.job(jid).await?;
                if chain["agent"] != self.bnb.agent_address(&intent.sender, &intent.agent_id)?
                    || chain["executor"] != intent.sender
                {
                    return Err(ApiError::unavailable(
                        "Bounty claim does not match this exact agent and wallet.",
                    ));
                }
                let claimant = self.store.agent(owner, &intent.agent_id)?;
                let mut db = self.store.connect()?;
                let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                let mut job = Store::job(&tx, jid)?;
                job.plan.executor_id = claimant.id;
                job.executor_name = claimant.plan.name;
                job.executor_wallet = intent.sender.clone();
                Store::save_job(&tx, &job)?;
                tx.commit()?;
            }
            let job = self.refresh_job(owner, jid, Some(&signature)).await?;
            let mut db = self.store.connect()?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let raw: String =
                tx.query_row("SELECT payload FROM bounties WHERE id=?", [bid], |r| {
                    r.get(0)
                })?;
            let mut bounty: Bounty = decode(raw)?;
            if bounty.job_id.as_deref() != Some(jid) {
                return Err(ApiError::conflict("Bounty terms changed."));
            }
            bounty.funding = job.funding;
            bounty.status = if intent.action == "claim_bounty" {
                "assigned"
            } else {
                "open"
            }
            .into();
            bounty.chain_tx = Some(signature.clone());
            if intent.action == "claim_bounty" {
                bounty.assigned_agent_id = Some(intent.agent_id.clone());
            }
            tx.execute(
                "UPDATE bounties SET payload=? WHERE id=?",
                params![encode(&bounty)?, bid],
            )?;
            tx.commit()?;
        }
        if ["bond_job", "settle_bond"].contains(&intent.action.as_str()) {
            let jid = intent.details["job_id"]
                .as_str()
                .ok_or_else(ApiError::internal)?;
            let job = self.get_job(owner, jid)?;
            let bond = self
                .bnb
                .view("economics", "getBond", vec![bnb::bytes32(jid)?])
                .await?;
            let token = required(&job.plan.bond_token_address, "Bond token is unavailable.")?;
            let decimals = self.bnb.token_decimals(token).await?;
            if bond["agent"]
                != self
                    .bnb
                    .agent_address(&intent.sender, &job.plan.executor_id)?
                || bond["owner"] != intent.sender
                || bond["token"] != token
                || bond["beneficiary"] != job.buyer_wallet
                || bnb::number(&bond["amount"])? != token_units(job.plan.bond_tokens, decimals)?
                || bnb::number(&bond["penaltyBps"])? != u128::from(job.plan.penalty_bps)
            {
                return Err(ApiError::unavailable(
                    "Onchain bond differs from the exact branch commitment.",
                ));
            }
            let status = match bnb::number(&bond["state"])? {
                0 => "locked",
                1 => "released",
                2 => "slashed",
                _ => return Err(ApiError::unavailable("Unknown bond state.")),
            };
            let mut db = self.store.connect()?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let mut fresh = Store::job(&tx, jid)?;
            if fresh.terms_hash != job.terms_hash {
                return Err(ApiError::conflict("Commitment terms changed."));
            }
            fresh.bond_status = status.into();
            Store::save_job(&tx, &fresh)?;
            tx.execute(
                "UPDATE bond_reservations SET status=?,tx_hash=? WHERE job_id=?",
                params![status, signature, jid],
            )?;
            tx.commit()?;
        }
        if intent.action == "pause_agent" {
            let chain = self
                .bnb
                .agent(&self.bnb.agent_address(&intent.sender, &intent.agent_id)?)
                .await?;
            let mut agent = self.store.agent(owner, &intent.agent_id)?;
            agent.status = if chain["paused"] == true {
                "paused"
            } else {
                "ready"
            }
            .into();
            self.store.save_agent(&agent)?;
        }
        if intent.details["finance_module"].as_str().is_some() {
            self.finance_record_confirmation(&intent).await?;
        }
        self.store
            .connect()?
            .execute(
                "UPDATE wallet_intents SET tx_hash=?,confirmed=1 WHERE id=?",
                params![signature, id],
            )
            .map_err(|_| {
                ApiError::conflict("Transaction is already attached to another wallet action.")
            })?;
        let agent = self.store.agent(owner, &intent.agent_id)?;
        if ["session_pay", "spend_credit"].contains(&intent.action.as_str()) {
            let amount = intent.details["amount"]
                .as_str()
                .ok_or_else(ApiError::internal)?;
            self.event(
                &agent,
                "payment",
                "merchant payment confirmed",
                "confirmed",
                intent.details["recipient"].as_str(),
                Some(amount),
                Some("USDT"),
                Some(&signature),
            )?;
        }
        self.event(
            &agent,
            "wallet_action",
            &format!("{} confirmed on BNB Smart Chain", intent.action),
            "confirmed",
            Some("bnb"),
            None,
            None,
            Some(&signature),
        )?;
        Ok(
            json!({"status":"confirmed","tx_hash":signature,"block_number":receipt["blockNumber"],"intent":intent}),
        )
    }
    pub fn x402_merchants(&self) -> Result<Vec<crate::x402::Merchant>> {
        if !self.config.x402_merchants.exists() {
            return Ok(vec![]);
        }
        let raw = std::fs::read(&self.config.x402_merchants)
            .map_err(|_| ApiError::unavailable("x402 merchant directory unavailable."))?;
        let merchants: Vec<crate::x402::Merchant> = serde_json::from_slice(&raw)
            .map_err(|_| ApiError::unavailable("Invalid x402 merchant directory."))?;
        if merchants.len() > 32 {
            return Err(ApiError::unavailable("Too many configured merchants."));
        }
        let mut ids = std::collections::HashSet::new();
        for m in &merchants {
            m.validate("eip155:56", &self.config.usdt)?;
            if !ids.insert(&m.id) {
                return Err(ApiError::unavailable("Duplicate merchant identifiers."));
            }
        }
        Ok(merchants)
    }
    pub async fn x402_system(&self) -> Result<Value> {
        let merchants = self.x402_merchants()?;
        let deployed = if merchants.is_empty() {
            false
        } else {
            self.bnb.deployed().await
        };
        let mut capabilities = vec![];
        for merchant in merchants {
            let supported = crate::x402::sponsor_supported(&merchant)
                .await
                .unwrap_or(false);
            capabilities.push(json!({"id":merchant.id,"recipient":merchant.recipient,"resource_url":merchant.resource_url,"capability_status":if supported{"quote_ready"}else{"unsupported"},"settlement_enabled":supported&&deployed}));
        }
        let enabled = capabilities.iter().any(|m| m["settlement_enabled"] == true);
        Ok(
            json!({"status":if capabilities.is_empty(){"not_connected"}else if enabled{"ready"}else{"configured"},"network":"eip155:56","currency":"USDT","asset":self.config.usdt,"merchants":capabilities,"settlement_enabled":enabled}),
        )
    }
}
