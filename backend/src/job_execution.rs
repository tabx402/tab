//! Execute funded jobs with the assigned agent's actual tools. Job outputs are
//! separate from public agent runs and never authorize escrow spending.
use crate::{
    bnb,
    db::{encode, Store},
    error::{ApiError, Result},
    models::*,
    AppState,
};
use chrono::{Duration, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::ToSchema;

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct JobRun {
    pub id: String,
    pub job_id: String,
    pub agent_id: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub output: Value,
}

fn executable(job: &Job, agent: &RuntimeAgent) -> Result<()> {
    if job.funding != "funded"
        || job.state != "open"
        || job.paused
        || job.plan.deadline <= Utc::now()
    {
        return Err(ApiError::conflict(
            "Only funded, open, unpaused jobs can run before their deadline.",
        ));
    }
    if agent.status != "ready" || agent.wallet.is_none() || agent.registry_id.is_none() {
        return Err(ApiError::conflict(
            "Register or resume the assigned executor first.",
        ));
    }
    if job.plan.executor_id != agent.id
        || job.executor_wallet != agent.wallet.as_deref().unwrap_or("")
        || job.plan.tools.is_empty()
        || job
            .plan
            .tools
            .iter()
            .any(|tool| !agent.plan.tools.contains(tool))
    {
        return Err(ApiError::conflict(
            "The assigned executor's current policy does not permit this job's tools.",
        ));
    }
    if digest(&job.terms) != job.terms_hash
        || job.terms["plan"] != serde_json::to_value(&job.plan)?
        || job.terms["executor_wallet"] != job.executor_wallet
        || job.terms["buyer_wallet"] != job.buyer_wallet
        || job.terms["id"] != job.id
        || job.terms["requester_id"] != job.requester_id
    {
        return Err(ApiError::unavailable("Saved job terms are inconsistent."));
    }
    Ok(())
}

fn narrowed_agent(agent: &RuntimeAgent, job: &Job) -> RuntimeAgent {
    let mut executor = agent.clone();
    executor.plan.purpose = job.plan.description.clone();
    executor.plan.tools = job.plan.tools.clone();
    executor.plan.max_call = agent.plan.max_call.min(job.plan.max_call);
    executor.plan.daily_cap = agent.plan.daily_cap.min(job.plan.budget);
    executor.plan.public_activity = false;
    executor
}

fn tool_status(tools: &Value) -> &'static str {
    let Some(output) = tools.as_object().filter(|output| !output.is_empty()) else {
        return "failed";
    };
    if output.iter().all(|(name, value)| {
        if name == "chain" {
            value["chain_id"] == 56
                && value["block"].as_u64().is_some()
                && value["wallet"].as_str().is_some()
                && value["bnb"].as_str().is_some()
                && value["usdt"].as_str().is_some()
        } else {
            matches!(value["status"].as_str(), Some("completed" | "confirmed"))
        }
    }) {
        "completed"
    } else {
        "partial"
    }
}

fn evidence(run: &JobRun) -> Value {
    let tools = &run.output["tools"];
    let mut observation = crate::runtime::safe_preview(tools).unwrap_or_else(|| json!({}));
    if let Some(summary) = observation["summary"].as_str() {
        observation["summary"] = json!(summary.chars().take(500).collect::<String>());
    }
    if let Some(sources) = observation["sources"].as_array() {
        let limited: Vec<_> = sources.iter().take(3).filter_map(|source| {
            let url = source["url"].as_str()?;
            if url.len() > 1024 {
                return None;
            }
            Some(json!({
                "url": url,
                "title": source["title"].as_str().unwrap_or("").chars().take(80).collect::<String>(),
                "excerpt": source["excerpt"].as_str().unwrap_or("").chars().take(100).collect::<String>(),
            }))
        }).collect();
        observation["sources"] = json!(limited);
    }
    // Paid response bodies remain in the private run record. The review proof
    // includes their verified payment and content commitment, not raw payloads.
    if tools["x402"]["status"] == "completed" {
        observation["paid_delivery"] = json!({
            "quote_id": tools["x402"]["quote_id"],
            "tx_hash": tools["x402"]["tx_hash"],
            "response_hash": tools["x402"]["response_hash"],
            "received_at": tools["x402"]["received_at"],
            "amount": tools["x402"]["amount"],
            "currency": tools["x402"]["currency"],
        });
    }
    json!({
        "type": "job_execution",
        "run_id": run.id,
        "job_id": run.job_id,
        "task": run.output["task"].as_str().unwrap_or("").chars().take(300).collect::<String>(),
        "result_hash": run.output["result_hash"],
        "observations": observation,
        "executed_at": run.finished_at,
        "verification": "buyer_review",
    })
}

impl AppState {
    pub fn job_runs(&self, owner: &str, id: &str) -> Result<Vec<JobRun>> {
        // Buyers and assigned executors have the same private job access as
        // existing job detail; unrelated accounts cannot discover run bodies.
        self.get_job(owner, id)?;
        let db = self.store.connect()?;
        let mut statement = db.prepare(
            "SELECT id,job_id,agent_id,status,started_at,finished_at,output FROM job_runs WHERE job_id=? ORDER BY started_at DESC LIMIT 20",
        )?;
        let rows = statement.query_map([id], |row| {
            Ok(JobRun {
                id: row.get(0)?,
                job_id: row.get(1)?,
                agent_id: row.get(2)?,
                status: row.get(3)?,
                started_at: row.get(4)?,
                finished_at: row.get(5)?,
                output: serde_json::from_str(&row.get::<_, String>(6)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    async fn verified_execution(&self, owner: &str, id: &str) -> Result<(Job, RuntimeAgent)> {
        let saved = self.get_job(owner, id)?;
        let agent = self.store.agent(owner, &saved.plan.executor_id)?;
        if agent.status != "ready" {
            return Err(ApiError::conflict(
                "Register or resume the assigned executor first.",
            ));
        }
        self.bnb.require_deployment().await?;
        self.verify_owned_agent(&agent).await?;
        let chain_agent = self
            .bnb
            .agent(agent.registry_id.as_deref().expect("verified agent"))
            .await?;
        if chain_agent["paused"] != false
            || chain_agent["policyHash"] != self.policy(&agent)
            || bnb::number(&chain_agent["dailyCap"])? != units(agent.plan.daily_cap)?
        {
            return Err(ApiError::conflict(
                "The executor's registered policy is paused or differs from its saved limits.",
            ));
        }
        let job = self.refresh_job(owner, id, None).await?;
        executable(&job, &agent)?;
        let chain_job = self.bnb.job(id).await?;
        if chain_job["buyer"] != job.buyer_wallet
            || job.buyer_wallet == bnb::ZERO
            || chain_job["executor"] != job.executor_wallet
            || chain_job["termsHash"] != job.terms_hash
            || bnb::number(&chain_job["state"])? != 1
            || chain_job["paused"] != false
        {
            return Err(ApiError::conflict(
                "The funded job's current BNB escrow state does not permit execution.",
            ));
        }
        if job.plan.tools.iter().any(|tool| tool == "bnb-rpc") {
            let head = bnb::number(&self.bnb.rpc("eth_blockNumber", json!([])).await?)?;
            if head.saturating_sub(u128::from(self.config.confirmations))
                < u128::from(job.plan.minimum_block)
            {
                return Err(ApiError::conflict(
                    "A confirmed BNB block meeting the job's minimum block is not available yet.",
                ));
            }
        }
        Ok((job, agent))
    }

    pub async fn run_job(&self, owner: &str, id: &str) -> Result<JobRun> {
        let assigned = self.get_job(owner, id)?;
        self.require_holder_agent(owner, &assigned.plan.executor_id)
            .await?;
        let (job, agent) = self.verified_execution(owner, id).await?;
        // Delivery lookup uses the original registered policy, before narrowing
        // the task and tools. Reusing data never sends another payment.
        let paid_delivery = if job.plan.tools.iter().any(|tool| tool == "x402") {
            self.paid_delivery_for_run(owner, &agent.id, None)?
        } else {
            None
        };
        let mut run = JobRun {
            id: identifier(),
            job_id: id.into(),
            agent_id: agent.id.clone(),
            status: "running".into(),
            started_at: now(),
            finished_at: None,
            output: json!({"job_id":id,"terms_hash":job.terms_hash,"task":job.plan.description,"tools":{},"evidence_status":"not_attached","job_escrow_payment_sent":false}),
        };
        {
            let mut db = self.store.connect()?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let current = Store::job(&tx, id)?;
            let current_agent = Store::agent_in(&tx, owner, &agent.id)?;
            executable(&current, &current_agent)?;
            if current.terms_hash != job.terms_hash
                || self.policy(&current_agent) != self.policy(&agent)
            {
                return Err(ApiError::conflict(
                    "Job or executor policy changed while preparing execution.",
                ));
            }
            let busy: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_runs WHERE agent_id=?1 AND status='running') OR EXISTS(SELECT 1 FROM job_runs WHERE (agent_id=?1 OR job_id=?2) AND status='running')",
                params![agent.id, id],
                |row| row.get(0),
            )?;
            if busy {
                return Err(ApiError::conflict(
                    "The assigned agent already has a running task.",
                ));
            }
            let recent: u64 = tx.query_row(
                "SELECT (SELECT count(*) FROM job_runs WHERE agent_id=?1 AND started_at>?2) + (SELECT count(*) FROM agent_runs WHERE agent_id=?1 AND started_at>?2)",
                params![agent.id, (Utc::now() - Duration::minutes(1)).to_rfc3339()],
                |row| row.get(0),
            )?;
            if recent > 0 {
                return Err(ApiError(
                    axum::http::StatusCode::TOO_MANY_REQUESTS,
                    "Wait one minute between job runs for this agent.".into(),
                ));
            }
            tx.execute(
                "INSERT INTO job_runs VALUES(?,?,?,?,?,?,?,?)",
                params![
                    run.id,
                    id,
                    agent.id,
                    owner,
                    run.status,
                    run.started_at,
                    run.finished_at,
                    encode(&run.output)?
                ],
            )?;
            tx.commit()?;
        }
        let narrowed = narrowed_agent(&agent, &job);
        match tokio::time::timeout(
            std::time::Duration::from_secs(90),
            self.execute_tools(&narrowed, &run.id, paid_delivery, false),
        )
        .await
        {
            Ok(Ok(tools)) => {
                run.status = tool_status(&tools).into();
                run.output["tools"] = tools;
                if run.status == "completed" && run.output["tools"]["chain"].is_object() {
                    let head = run.output["tools"]["chain"]["block"]
                        .as_u64()
                        .expect("validated chain output");
                    let number = head.saturating_sub(self.config.confirmations);
                    let observation = self
                        .bnb
                        .rpc(
                            "eth_getBlockByNumber",
                            json!([format!("0x{number:x}"), false]),
                        )
                        .await;
                    match observation {
                        Ok(block)
                            if number >= job.plan.minimum_block
                                && bnb::number(&block["number"]).ok()
                                    == Some(u128::from(number))
                                && block["hash"]
                                    .as_str()
                                    .is_some_and(|hash| bnb::signature(hash).is_ok()) =>
                        {
                            run.output["tools"]["chain"]["confirmed_block"] =
                                json!({"number":number,"hash":block["hash"],"observed_at":now()});
                        }
                        _ => {
                            run.status = "partial".into();
                            run.output["error"] = json!("The wallet observation was returned, but its qualifying confirmed block could not be verified.");
                        }
                    }
                }
            }
            Ok(Err(error)) => {
                run.status = "failed".into();
                run.output["error"] = json!(error.1);
            }
            Err(_) => {
                run.status = "failed".into();
                run.output["error"] = json!(
                    "Job tools exceeded their execution time limit. No escrow payment was sent."
                );
            }
        }
        run.finished_at = Some(now());
        let result = json!({"job_id":id,"terms_hash":job.terms_hash,"task":job.plan.description,"tools":run.output["tools"]});
        run.output["result_hash"] = json!(digest(&result));
        // Keep the cross-run reservation until evidence processing finishes,
        // while making its referenced result durable before committing proof.
        // Startup recovery can retain this output if the process stops here.
        let persisted = self.store.connect()?.execute(
            "UPDATE job_runs SET finished_at=?,output=? WHERE id=? AND owner=? AND status='running'",
            params![run.finished_at, encode(&run.output)?, run.id, owner],
        )?;
        if persisted != 1 {
            return Err(ApiError::conflict("The job execution reservation changed before its result was saved. No evidence was attached."));
        }
        if run.status == "completed" {
            match self.verified_execution(owner, id).await {
                Ok((fresh, fresh_agent))
                    if fresh.terms_hash == job.terms_hash
                        && self.policy(&fresh_agent) == self.policy(&agent) =>
                {
                    match self.attach_job_run_evidence(owner, id, &job.terms_hash, evidence(&run)) {
                        Ok(attached) => {
                            run.output["evidence_status"] = json!("attached");
                            run.output["evidence_hash"] = json!(attached.evidence_hash);
                        }
                        Err(error) => {
                            run.status = "partial".into();
                            run.output["error"] = json!(error.1);
                        }
                    }
                }
                Ok(_) => {
                    run.status = "partial".into();
                    run.output["error"] = json!("Job terms or executor policy changed during execution. The output is saved without attached evidence.");
                }
                Err(error) => {
                    run.status = "partial".into();
                    run.output["error"] = json!(error.1);
                }
            }
        }
        self.store.connect()?.execute(
            "UPDATE job_runs SET status=?,finished_at=?,output=? WHERE id=? AND owner=?",
            params![
                run.status,
                run.finished_at,
                encode(&run.output)?,
                run.id,
                owner
            ],
        )?;
        Ok(run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethabi::Token;

    const WALLET: &str = "0x4444444444444444444444444444444444444444";
    fn call(signature: &str) -> String {
        format!("call:0x{}", &bnb::hash(signature.as_bytes())[2..10])
    }
    fn encoded(tokens: &[Token]) -> Value {
        json!(format!("0x{}", hex::encode(ethabi::encode(tokens))))
    }
    fn chain_agent(state: &AppState, agent: &RuntimeAgent, paused: bool) -> Value {
        encoded(&[Token::Tuple(vec![
            bnb::addr(WALLET).unwrap(),
            Token::String(agent.plan.name.clone()),
            bnb::uint(units(agent.plan.daily_cap).unwrap()),
            bnb::bytes32(&state.policy(agent)).unwrap(),
            Token::Bool(paused),
            bnb::uint(1),
            bnb::uint(0),
            bnb::uint(0),
        ])])
    }
    fn chain_job(state: &AppState, job: &Job, exists: bool, paused: bool, terms: &str) -> Value {
        encoded(&[Token::Tuple(vec![
            bnb::bytes32(&bnb::id(&job.root_id).unwrap()).unwrap(),
            bnb::bytes32(&bnb::ZERO_HASH).unwrap(),
            bnb::addr(if exists { &job.buyer_wallet } else { bnb::ZERO }).unwrap(),
            bnb::addr(&job.executor_wallet).unwrap(),
            bnb::bytes32(
                &state
                    .bnb
                    .agent_address(&job.executor_wallet, &job.plan.executor_id)
                    .unwrap(),
            )
            .unwrap(),
            bnb::uint(units(job.plan.budget).unwrap()),
            bnb::uint(units(job.plan.budget).unwrap()),
            bnb::uint(units(job.plan.max_call).unwrap()),
            bnb::uint(job.plan.deadline.timestamp() as u128),
            bnb::bytes32(terms).unwrap(),
            bnb::bytes32(bnb::ZERO_HASH).unwrap(),
            bnb::uint(u128::from(tool_bitmap(&job.plan.tools))),
            bnb::uint(0),
            bnb::uint(0),
            bnb::uint(0),
            bnb::uint(0),
            bnb::uint(1),
            Token::Bool(paused),
            Token::Bool(false),
            bnb::uint(200),
            bnb::uint(0),
            bnb::uint(0),
            bnb::uint(0),
            bnb::uint(0),
            Token::Array(vec![]),
        ])])
    }
    async fn fixture(tools: &[&str]) -> (crate::chain_tests::Fixture, Job, RuntimeAgent) {
        let f = crate::chain_tests::fixture().await;
        let make_agent = |owner: &str, name: &str| {
            let mut agent = f.state.create_agent(owner, serde_json::from_value(json!({
                "name":name,"purpose":"the default agent purpose must not replace a job task",
                "tools":tools,"daily_cap":"10","max_call":"0.1","public_activity":true
            })).unwrap()).unwrap();
            agent.wallet = Some(WALLET.into());
            agent.registry_id = Some(f.state.bnb.agent_address(WALLET, &agent.id).unwrap());
            agent.status = "ready".into();
            f.state.store.save_agent(&agent).unwrap();
            agent
        };
        let buyer = make_agent("buyer", "buyer");
        let executor = make_agent("executor", "executor");
        let plan: JobInput = serde_json::from_value(json!({
            "title":"inspect the funded escrow wallet",
            "description":"observe this job executor's actual BNB and USDT balances for buyer review",
            "executor_id":executor.id,"budget":"2","max_call":"0.05",
            "deadline":(Utc::now()+Duration::hours(4)).to_rfc3339(),"tools":tools,"public_activity":false
        })).unwrap();
        let job = f
            .state
            .create_job("buyer", &buyer.id, plan, None)
            .await
            .unwrap();
        {
            let mut replies = f.replies.lock().unwrap();
            replies.insert(
                call("getAgent(bytes32)"),
                chain_agent(&f.state, &executor, false),
            );
            replies.insert(
                call("getJob(bytes32)"),
                chain_job(&f.state, &job, true, false, &job.terms_hash),
            );
            replies.insert("eth_getBalance".into(), json!("0x58d15e176280000"));
            replies.insert(
                "eth_getBlockByNumber".into(),
                json!({"number":"0x63","hash":format!("0x{}","66".repeat(32))}),
            );
            replies.insert(
                call("balanceOf(address)"),
                encoded(&[bnb::uint(3_000_000_000_000_000_000)]),
            );
        }
        (f, job, executor)
    }
    #[tokio::test]
    async fn funded_job_uses_its_task_and_real_rpc_outputs_without_public_leaks() {
        let (f, job, executor) = fixture(&["bnb-rpc"]).await;
        let before = f.state.events(None, None, 0, 100).unwrap().len();
        let run = f.state.run_job("executor", &job.id).await.unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(run.output["task"], job.plan.description);
        assert_ne!(run.output["task"], executor.plan.purpose);
        assert_eq!(run.output["tools"]["chain"]["wallet"], WALLET);
        assert_eq!(run.output["tools"]["chain"]["usdt"], "3");
        assert_eq!(run.output["evidence_status"], "attached");
        assert_eq!(run.output["job_escrow_payment_sent"], false);
        let saved = f.state.get_job("executor", &job.id).unwrap();
        assert_eq!(saved.state, "open");
        assert_eq!(saved.provider_paid, rust_decimal::Decimal::ZERO);
        assert_eq!(saved.evidence.as_ref().unwrap()["output"]["run_id"], run.id);
        assert_eq!(
            saved.evidence.as_ref().unwrap()["output"]["result_hash"],
            run.output["result_hash"]
        );
        assert_eq!(
            f.state.job_runs("executor", &job.id).unwrap()[0].output,
            run.output
        );
        assert_eq!(f.state.job_runs("buyer", &job.id).unwrap()[0].id, run.id);
        assert!(f.state.job_runs("unrelated", &job.id).is_err());
        assert!(f.state.store.runs(&executor.id).unwrap().is_empty());
        assert_eq!(f.state.events(None, None, 0, 100).unwrap().len(), before);
        assert!(f.state.public_jobs().unwrap().is_empty());
        let replies = f.replies.lock().unwrap();
        assert_eq!(replies["request:eth_getBalance"]["params"][0], WALLET);
        let observed = run.output["tools"]["chain"]["block"].as_u64().unwrap();
        let block = format!("0x{observed:x}");
        assert_eq!(replies["request:eth_getBalance"]["params"][1], block);
        for signature in ["decimals()", "balanceOf(address)"] {
            let request = &replies[&format!("request:{}:block:{block}", call(signature))];
            assert_eq!(request["params"][0]["to"], bnb::USDT);
            assert_eq!(request["params"][1], block);
        }
        assert_eq!(
            run.output["tools"]["chain"]["confirmed_block"]["number"],
            observed - f.state.config.confirmations
        );
        assert!(replies.get("sent").is_none());
    }
    #[tokio::test]
    async fn completed_evidence_requires_a_durable_full_result_before_attachment() {
        let (f, job, _executor) = fixture(&["bnb-rpc"]).await;
        f.state.store.connect().unwrap().execute_batch(
            "CREATE TRIGGER durable_job_result BEFORE UPDATE OF payload ON jobs
            WHEN json_extract(NEW.payload,'$.evidence.output.type')='job_execution'
            BEGIN
              SELECT CASE WHEN NOT EXISTS(
                SELECT 1 FROM job_runs
                WHERE id=json_extract(NEW.payload,'$.evidence.output.run_id')
                AND status='running'
                AND finished_at IS NOT NULL
                AND json_extract(output,'$.result_hash')=json_extract(NEW.payload,'$.evidence.output.result_hash')
                AND json_extract(output,'$.tools.chain.usdt')='3'
              ) THEN RAISE(ABORT,'Job result must be durable before evidence') END;
            END;"
        ).unwrap();
        let run = f.state.run_job("executor", &job.id).await.unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(run.output["evidence_status"], "attached");
        let saved = f.state.job_runs("executor", &job.id).unwrap();
        assert_eq!(saved[0].output, run.output);
        assert_eq!(saved[0].status, "completed");
        assert_eq!(
            f.state
                .get_job("buyer", &job.id)
                .unwrap()
                .evidence
                .as_ref()
                .unwrap()["output"]["result_hash"],
            saved[0].output["result_hash"]
        );
    }
    #[tokio::test]
    async fn wrong_executor_and_unfunded_jobs_do_not_run_tools_or_create_history() {
        let (f, job, _executor) = fixture(&["bnb-rpc"]).await;
        assert!(f.state.run_job("buyer", &job.id).await.is_err());
        f.replies.lock().unwrap().insert(
            call("getJob(bytes32)"),
            chain_job(&f.state, &job, false, false, &job.terms_hash),
        );
        assert!(f.state.run_job("executor", &job.id).await.is_err());
        assert!(f.state.job_runs("executor", &job.id).unwrap().is_empty());
        assert!(f
            .replies
            .lock()
            .unwrap()
            .get("request:eth_getBalance")
            .is_none());
    }
    #[tokio::test]
    async fn paused_policy_or_job_and_inconsistent_saved_terms_fail_before_tools() {
        let (f, job, executor) = fixture(&["bnb-rpc"]).await;
        f.replies.lock().unwrap().insert(
            call("getAgent(bytes32)"),
            chain_agent(&f.state, &executor, true),
        );
        assert!(f.state.run_job("executor", &job.id).await.is_err());
        f.replies.lock().unwrap().insert(
            call("getAgent(bytes32)"),
            chain_agent(&f.state, &executor, false),
        );
        f.replies.lock().unwrap().insert(
            call("getJob(bytes32)"),
            chain_job(&f.state, &job, true, true, &job.terms_hash),
        );
        assert!(f.state.run_job("executor", &job.id).await.is_err());
        f.replies.lock().unwrap().insert(
            call("getJob(bytes32)"),
            chain_job(&f.state, &job, true, false, &job.terms_hash),
        );
        let mut tampered = job.clone();
        tampered.plan.description = "a different task without buyer-approved terms".into();
        Store::save_job(&f.state.store.connect().unwrap(), &tampered).unwrap();
        assert!(f.state.run_job("executor", &job.id).await.is_err());
        assert!(f.state.job_runs("executor", &job.id).unwrap().is_empty());
        assert!(f
            .replies
            .lock()
            .unwrap()
            .get("request:eth_getBalance")
            .is_none());
    }
    #[tokio::test]
    async fn partial_provider_run_saves_actual_observations_without_completed_evidence() {
        let (f, job, executor) = fixture(&["bnb-rpc", "openrouter"]).await;
        let before = encode(&f.state.public_records().unwrap()).unwrap();
        let run = f.state.run_job("executor", &job.id).await.unwrap();
        assert_eq!(run.status, "partial");
        assert_eq!(run.output["tools"]["chain"]["usdt"], "3");
        assert_eq!(run.output["tools"]["openrouter"]["status"], "not_connected");
        assert_eq!(run.output["evidence_status"], "not_attached");
        assert!(f
            .state
            .get_job("executor", &job.id)
            .unwrap()
            .evidence
            .is_none());
        assert!(f.state.store.runs(&executor.id).unwrap().is_empty());
        assert_eq!(encode(&f.state.public_records().unwrap()).unwrap(), before);
    }
    #[tokio::test]
    async fn agent_and_job_execution_share_a_concurrency_guard() {
        let (f, job, executor) = fixture(&["bnb-rpc"]).await;
        let db = f.state.store.connect().unwrap();
        db.execute(
            "INSERT INTO agent_runs VALUES(?,?,?,?,?,?)",
            params![
                "existing",
                executor.id,
                "running",
                now(),
                Option::<String>::None,
                "{}"
            ],
        )
        .unwrap();
        assert!(f.state.run_job("executor", &job.id).await.is_err());
        db.execute("DELETE FROM agent_runs", []).unwrap();
        db.execute(
            "INSERT INTO job_runs VALUES(?,?,?,?,?,?,?,?)",
            params![
                "existing",
                job.id,
                executor.id,
                "executor",
                "running",
                now(),
                Option::<String>::None,
                "{}"
            ],
        )
        .unwrap();
        assert!(f.state.run_job("executor", &job.id).await.is_err());
        assert!(f.state.run_agent("executor", &executor.id).await.is_err());
        assert!(f
            .replies
            .lock()
            .unwrap()
            .get("request:eth_getBalance")
            .is_none());
    }
    #[tokio::test]
    async fn chain_terms_changing_during_execution_preserves_output_but_attaches_no_evidence() {
        let (f, job, _executor) = fixture(&["bnb-rpc"]).await;
        f.replies
            .lock()
            .unwrap()
            .insert("delay:eth_getBalance".into(), json!(120));
        let state = f.state.clone();
        let id = job.id.clone();
        let running = tokio::spawn(async move { state.run_job("executor", &id).await });
        // The fixture records a call after its delay. job_runs becomes running
        // before that request, so wait for the durable concurrency reservation.
        for _ in 0..100 {
            if f.state
                .job_runs("executor", &job.id)
                .unwrap()
                .iter()
                .any(|run| run.status == "running")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let changed = format!("0x{}", "aa".repeat(32));
        f.replies.lock().unwrap().insert(
            call("getJob(bytes32)"),
            chain_job(&f.state, &job, true, false, &changed),
        );
        let run = running.await.unwrap().unwrap();
        assert_eq!(run.status, "partial");
        assert_eq!(run.output["tools"]["chain"]["usdt"], "3");
        assert_eq!(run.output["evidence_status"], "not_attached");
        assert!(f
            .state
            .get_job("executor", &job.id)
            .unwrap()
            .evidence
            .is_none());
        assert!(run.output["error"].as_str().unwrap().contains("terms"));
    }
    #[tokio::test]
    async fn minimum_block_and_expired_deadline_reject_work_before_provider_calls() {
        let (f, mut job, _executor) = fixture(&["bnb-rpc"]).await;
        job.plan.minimum_block = 1000;
        job.terms["plan"] = serde_json::to_value(&job.plan).unwrap();
        job.terms_hash = digest(&job.terms);
        Store::save_job(&f.state.store.connect().unwrap(), &job).unwrap();
        f.replies.lock().unwrap().insert(
            call("getJob(bytes32)"),
            chain_job(&f.state, &job, true, false, &job.terms_hash),
        );
        assert!(f
            .state
            .run_job("executor", &job.id)
            .await
            .unwrap_err()
            .1
            .contains("minimum block"));
        job.plan.minimum_block = 1;
        job.plan.deadline = Utc::now() - Duration::seconds(1);
        job.terms["plan"] = serde_json::to_value(&job.plan).unwrap();
        job.terms_hash = digest(&job.terms);
        Store::save_job(&f.state.store.connect().unwrap(), &job).unwrap();
        f.replies.lock().unwrap().insert(
            call("getJob(bytes32)"),
            chain_job(&f.state, &job, true, false, &job.terms_hash),
        );
        assert!(f.state.run_job("executor", &job.id).await.is_err());
        assert!(f.state.job_runs("executor", &job.id).unwrap().is_empty());
        assert!(f
            .replies
            .lock()
            .unwrap()
            .get("request:eth_getBalance")
            .is_none());
    }
    #[tokio::test]
    async fn builder_run_and_history_are_scoped_to_the_exact_executor_key() {
        use axum::{
            body::Body,
            http::{Request, StatusCode},
        };
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let (f, job, executor) = fixture(&["bnb-rpc"]).await;
        let wrong = f.state.create_key("buyer", &job.requester_id).unwrap();
        let key = f.state.create_key("executor", &executor.id).unwrap();
        let mut state = f.state.clone();
        state.auth = crate::auth::Auth::new(Some("job-test-app".into()), state.client.clone());
        let app = crate::api::router(state);
        for suffix in ["run", "runs"] {
            let request = Request::builder()
                .method(if suffix == "run" { "POST" } else { "GET" })
                .uri(format!("/api/agent/jobs/{}/{suffix}", job.id))
                .header("Authorization", format!("Bearer {wrong}"))
                .body(Body::empty())
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert_eq!(response.headers()["Cache-Control"], "no-store");
        }
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/agent/jobs/{}/run", job.id))
            .header("Authorization", format!("Bearer {key}"))
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["status"], "completed");
        let request = Request::builder()
            .uri(format!("/api/agent/jobs/{}/runs", job.id))
            .header("Authorization", format!("Bearer {key}"))
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body[0]["agent_id"], executor.id);
        let request = Request::builder()
            .uri(format!("/api/account/jobs/{}/runs", job.id))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }
    #[test]
    fn job_policy_narrows_tools_and_budgets_and_only_known_tool_success_completes() {
        let (_dir, state) = crate::tests::state();
        let agent = state.create_agent("executor", serde_json::from_value(json!({"name":"executor","purpose":"generic purpose","tools":["bnb-rpc","openrouter"],"daily_cap":"10","max_call":"0.1"})).unwrap()).unwrap();
        let job: Job = serde_json::from_value(json!({
            "title":"a funded job","description":"the buyer's specific job task","executor_id":agent.id,"budget":"2","max_call":"0.05",
            "deadline":(Utc::now()+Duration::hours(1)).to_rfc3339(),"tools":["bnb-rpc"],"id":"x","root_id":"x","parent_id":null,"requester_id":"y",
            "buyer_wallet":WALLET,"executor_wallet":WALLET,"executor_name":"executor","created_at":now(),"depth":0,"terms_hash":"x","state":"open","funding":"funded",
            "available":"2","evidence_hash":null,"evidence":null,"paused":false,"chain_tx":null,"provider_paid":"0","reward_paid":"0","refunded":"0","root_services":[]
        })).unwrap();
        let narrowed = narrowed_agent(&agent, &job);
        assert_eq!(narrowed.plan.purpose, job.plan.description);
        assert_eq!(narrowed.plan.tools, vec!["bnb-rpc"]);
        assert_eq!(narrowed.plan.max_call.to_string(), "0.05");
        assert_eq!(narrowed.plan.daily_cap.to_string(), "2");
        assert!(!narrowed.plan.public_activity);
        assert_eq!(tool_status(&json!({})), "failed");
        for status in [
            "requires_authorization",
            "not_connected",
            "unavailable",
            "unknown",
            "failed_after_payment",
        ] {
            assert_eq!(tool_status(&json!({"x402":{"status":status}})), "partial");
        }
    }
}
