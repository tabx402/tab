use crate::{
    auth::{Builder, Owner},
    bnb,
    db::encode,
    error::{ApiError, Result},
    models::*,
    AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{Html, Response},
    routing::{get, post},
    Json, Router,
};
use rusqlite::params;
use serde::Deserialize;
use serde_json::{json, Value};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/config", get(config))
        .route(
            "/api/account/profile",
            get(account_profile).post(register_account),
        )
        .route("/api/credit/assets", get(credit_assets))
        .route("/api/sponsorship", get(sponsorship))
        .route("/api/providers", get(providers))
        .route("/api/overview", get(overview))
        .route("/api/registry", get(registry))
        .route("/api/starter/wallet", get(starter_wallet))
        .route("/api/operators", get(operators))
        .route("/api/finance/system", get(finance_system))
        .route("/api/finance/quote", post(finance_quote))
        .route("/api/account/runtime/{id}/finance", get(finance_account))
        .route(
            "/api/account/runtime/{id}/finance/prepare",
            post(finance_prepare),
        )
        .route(
            "/api/account/runtime/{id}/finance/requests",
            get(finance_requests).post(finance_request),
        )
        .route("/api/agents/live/{id}/runs/{run}", get(public_run_receipt))
        .route("/api/activity", get(activity))
        .route("/api/agents/live", get(live_agents))
        .route("/api/agents/records", get(records))
        .route("/api/agents/live/{id}", get(live_agent))
        .route("/api/account/agents", get(plans).post(create_plan))
        .route(
            "/api/account/agents/{id}",
            axum::routing::delete(delete_plan),
        )
        .route(
            "/api/account/runtime",
            get(runtime_agents).post(runtime_create),
        )
        .route(
            "/api/account/runtime/{id}",
            axum::routing::patch(runtime_update),
        )
        .route("/api/account/runtime/{id}/challenge", post(challenge))
        .route("/api/account/runtime/{id}/register", post(register))
        .route("/api/account/runtime/{id}/sponsor", post(sponsor))
        .route(
            "/api/account/runtime/{id}/sponsor/status",
            post(sponsor_status),
        )
        .route("/api/account/runtime/{id}/pause", post(pause))
        .route("/api/account/runtime/{id}/run", post(run))
        .route("/api/account/runtime/{id}/runs", get(runs))
        .route("/api/account/runtime/{id}/events", get(events))
        .route(
            "/api/account/runtime/{id}/key",
            post(key).delete(revoke_key),
        )
        .route("/api/agent/run", post(builder_run))
        .route("/api/account/runtime/{id}/balance", get(balance))
        .route(
            "/api/account/runtime/{id}/purchase/quote",
            post(purchase_quote),
        )
        .route(
            "/api/account/runtime/{id}/purchase/{quote_id}",
            post(purchase),
        )
        .route(
            "/api/account/runtime/{id}/purchase/{quote_id}/status",
            post(purchase_status),
        )
        .route("/api/account/runtime/{id}/purchases", get(purchases))
        .route("/api/tools", get(tools))
        .route("/api/capabilities", get(capabilities))
        .route("/api/models", get(models))
        .route("/api/feed", get(feed))
        .route("/api/bounties", get(bounties))
        .route(
            "/api/account/bounties",
            get(account_bounties).post(create_bounty),
        )
        .route("/api/metrics", get(metrics))
        .route("/api/token/system", get(token_system))
        .route("/api/account/runtime/{id}/eligibility", get(eligibility))
        .route("/api/account/runtime/{id}/credit", get(credit_accounts))
        .route(
            "/api/account/runtime/{id}/wallet-actions",
            get(wallet_actions),
        )
        .route("/api/backing/assets", get(backing_assets))
        .route("/api/assets/backing", get(backing_assets))
        .route("/api/x402/system", get(x402_system))
        .route("/api/account/runtime/{id}/x402", get(x402_history))
        .route("/api/account/runtime/{id}/x402/quote", post(x402_quote))
        .route(
            "/api/account/runtime/{id}/x402/{quote_id}/execute",
            post(x402_execute),
        )
        .route(
            "/api/account/runtime/{id}/x402/{quote_id}/reconcile",
            post(x402_reconcile),
        )
        .route(
            "/api/account/runtime/{id}/x402/{quote_id}/release-expired",
            post(x402_release_expired),
        )
        .route(
            "/api/account/runtime/{id}/wallet-actions/prepare",
            post(financial_prepare),
        )
        .route(
            "/api/account/wallet-actions/{id}/confirm",
            post(financial_confirm),
        )
        .route(
            "/api/account/wallet-actions/{id}/submitted",
            post(financial_submitted),
        )
        .route(
            "/api/account/wallet-actions/{id}/release-failed",
            post(financial_release_failed),
        )
        .route(
            "/api/account/job-actions/{id}/submitted",
            post(job_submitted),
        )
        .route(
            "/api/account/job-actions/{id}/release-failed",
            post(job_release_failed),
        )
        .route("/api/jobs/system", get(job_system))
        .route("/api/jobs/services", get(job_services))
        .route("/api/jobs", get(public_jobs))
        .route("/api/account/jobs", get(account_jobs))
        .route("/api/account/job-actions", get(job_actions))
        .route("/api/account/jobs/{id}", get(account_job))
        .route("/api/account/runtime/{id}/jobs", post(create_job))
        .route("/api/account/jobs/{id}/branches", post(create_branch))
        .route(
            "/api/account/jobs/{id}/evidence",
            post(collect_evidence).put(attach_evidence),
        )
        .route("/api/account/jobs/{id}/cancel-draft", post(cancel_draft))
        .route("/api/account/jobs/{id}/prepare", post(prepare_job))
        .route("/api/account/job-actions/{id}/confirm", post(confirm_job))
        .route("/api/account/jobs/{id}/refresh", post(refresh_job))
        .route("/api/agent/jobs", get(builder_jobs))
        .route("/api/agent/jobs/{id}/branches", post(builder_branch))
        .route("/api/agent/jobs/{id}/evidence", post(builder_evidence))
        .route("/api/bnb/transactions/{signature}", get(transaction_status))
        .route("/api/bnb/tokens/{address}", get(token_metadata))
        .route("/api/openapi.json", get(openapi))
        .route("/api/docs", get(docs))
        .layer(axum::extract::DefaultBodyLimit::max(32_768))
        .layer(middleware::from_fn(private_cache))
        .with_state(state)
}
async fn private_cache(request: axum::extract::Request, next: Next) -> Response {
    let private = request.uri().path().starts_with("/api/account/")
        || request.uri().path().starts_with("/api/agent/");
    let mut response = next.run(request).await;
    if private {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
    }
    response
}
async fn health(State(state): State<AppState>) -> Result<Json<Health>> {
    state
        .store
        .connect()?
        .query_row("SELECT 1", [], |_| Ok(()))?;
    Ok(Json(Health {
        status: "ok".into(),
        financial_actions_enabled: state.bnb.deployed().await,
        backend: "rust".into(),
    }))
}
async fn config(State(state): State<AppState>) -> Json<PublicConfig> {
    let sponsorship = state.sponsorship().await;
    Json(PublicConfig {
        app_id: state.config.app_id.clone(),
        financial_actions_enabled: state.bnb.deployed().await,
        contracts_status: if state.bnb.deployed().await {
            "live"
        } else {
            "not_deployed"
        }
        .into(),
        chain_id: 56,
        network: state.config.network.clone(),
        gas_sponsorship_enabled: sponsorship["status"] == "ready",
        gas_sponsorship_status: sponsorship["status"]
            .as_str()
            .unwrap_or("unavailable")
            .into(),
        gas_sponsorship_message: sponsorship["message"]
            .as_str()
            .unwrap_or("Registration sponsorship is unavailable.")
            .into(),
        payments_enabled: state.bnb.deployed().await,
        agent_execution_enabled: true,
        usdt_address: state.config.usdt.clone(),
        usdt_decimals: 18,
        official_tab_address: state.config.official_tab.clone(),
        preferred_wallet: "metamask".into(),
        backend: "rust".into(),
    })
}
async fn sponsorship(State(state): State<AppState>) -> Json<Value> {
    Json(state.sponsorship().await)
}
#[derive(Deserialize)]
struct StarterQuery {
    address: String,
}
async fn starter_wallet(
    State(state): State<AppState>,
    Query(query): Query<StarterQuery>,
) -> Result<Json<crate::starter::StarterObservation>> {
    Ok(Json(state.starter_wallet(&query.address).await?))
}
async fn operators(State(state): State<AppState>) -> Result<Json<Value>> {
    Ok(Json(state.operator_records()?))
}
async fn public_run_receipt(
    State(state): State<AppState>,
    Path((agent, run)): Path<(String, String)>,
) -> Result<Json<Value>> {
    Ok(Json(state.public_run_receipt(&agent, &run)?))
}
async fn providers(State(state): State<AppState>) -> Json<Vec<Provider>> {
    Json(state.providers().await)
}
#[derive(Deserialize, Default)]
struct OverviewQuery {
    mode: Option<String>,
}
async fn overview(Query(query): Query<OverviewQuery>) -> Result<Json<PublicData>> {
    if query
        .mode
        .as_deref()
        .is_some_and(|m| !["public", "example"].contains(&m))
    {
        return Err(ApiError::validation("Choose public or example mode."));
    }
    // Illustrated activity is removed from the primary economic feed.
    Ok(Json(PublicData {
        mode: query.mode.unwrap_or_else(|| "public".into()),
        summary: Summary::default(),
        agents: vec![],
        receipts: vec![],
        series: vec![],
    }))
}
async fn plans(State(state): State<AppState>, Owner(owner): Owner) -> Result<Json<Vec<AgentPlan>>> {
    Ok(Json(state.store.list_plans(&owner)?))
}
async fn create_plan(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Json(plan): Json<AgentPlanInput>,
) -> Result<(StatusCode, Json<AgentPlan>)> {
    let plan = plan.validate()?;
    let allowed: Vec<_> = state.providers().await.into_iter().map(|p| p.id).collect();
    if plan.providers.iter().any(|p| !allowed.contains(p)) {
        return Err(ApiError::validation("Choose providers from the directory."));
    }
    let plan = AgentPlan {
        plan,
        id: identifier(),
        created_at: now(),
        status: "draft".into(),
    };
    let mut db = state.store.connect()?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let count: u64 = tx.query_row(
        "SELECT count(*) FROM agent_plans WHERE owner=?",
        [&owner],
        |r| r.get(0),
    )?;
    if count >= 20 {
        return Err(ApiError::conflict(
            "Your account can hold up to 20 agent plans.",
        ));
    }
    tx.execute(
        "INSERT INTO agent_plans VALUES(?,?,?,?)",
        params![plan.id, owner, encode(&plan)?, plan.created_at],
    )?;
    tx.commit()?;
    Ok((StatusCode::CREATED, Json(plan)))
}
async fn delete_plan(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    if state.store.connect()?.execute(
        "DELETE FROM agent_plans WHERE id=? AND owner=?",
        params![id, owner],
    )? != 1
    {
        return Err(ApiError::missing("Agent plan not found."));
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn runtime_agents(
    State(state): State<AppState>,
    Owner(owner): Owner,
) -> Result<Json<Vec<RuntimeAgent>>> {
    Ok(Json(state.store.list_agents(&owner)?))
}
async fn runtime_create(
    State(state): State<AppState>,
    Owner(owner): Owner,
    headers: axum::http::HeaderMap,
    Json(plan): Json<AgentInput>,
) -> Result<(StatusCode, Json<RuntimeAgent>)> {
    let key = headers
        .get("idempotency-key")
        .map(|v| {
            v.to_str()
                .map_err(|_| ApiError::validation("Invalid creation request key."))
        })
        .transpose()?;
    Ok((
        StatusCode::CREATED,
        Json(state.create_agent_once(&owner, plan, key)?),
    ))
}
async fn account_profile(
    State(state): State<AppState>,
    Owner(owner): Owner,
) -> Result<Json<Value>> {
    Ok(Json(state.account_profile(&owner)?))
}
async fn register_account(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Json(input): Json<AccountInput>,
) -> Result<Json<Value>> {
    Ok(Json(state.register_account(&owner, input)?))
}
async fn credit_assets(State(state): State<AppState>) -> Result<Json<Value>> {
    Ok(Json(state.secured_assets().await?))
}
async fn runtime_update(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Json(plan): Json<AgentInput>,
) -> Result<Json<RuntimeAgent>> {
    Ok(Json(state.update_agent(&owner, &id, plan)?))
}
async fn challenge(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Json(proof): Json<WalletChallenge>,
) -> Result<Json<Value>> {
    if proof.sponsored.unwrap_or(true) {
        Ok(Json(serde_json::to_value(
            state
                .sponsored_challenge(&owner, &id, &proof.wallet)
                .await?,
        )?))
    } else {
        Ok(Json(state.challenge(&owner, &id, &proof.wallet).await?))
    }
}
async fn register(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Json(proof): Json<WalletProof>,
) -> Result<Json<RuntimeAgent>> {
    Ok(Json(state.register(&owner, &id, proof).await?))
}
async fn sponsor(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Json(proof): Json<RegistrationSignature>,
) -> Result<Json<SponsorshipResult>> {
    Ok(Json(state.sponsor_registration(&owner, &id, proof).await?))
}
async fn sponsor_status(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<SponsorshipResult>> {
    Ok(Json(state.sponsor_status(&owner, &id).await?))
}
async fn pause(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Json(input): Json<PauseInput>,
) -> Result<Json<RuntimeAgent>> {
    Ok(Json(state.toggle_agent(&owner, &id, input.paused)?))
}
async fn run(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<AgentRun>> {
    Ok(Json(state.run_agent(&owner, &id).await?))
}
async fn builder_run(
    State(state): State<AppState>,
    Builder(owner, id): Builder,
) -> Result<Json<AgentRun>> {
    Ok(Json(state.run_agent(&owner, &id).await?))
}
async fn runs(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Vec<AgentRun>>> {
    state.store.agent(&owner, &id)?;
    Ok(Json(state.store.runs(&id)?))
}
async fn events(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Vec<AgentEvent>>> {
    state.store.agent(&owner, &id)?;
    Ok(Json(state.events(Some(&owner), Some(&id), 0, 100)?))
}
async fn key(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(json!({"key":state.create_key(&owner,&id)?})))
}
async fn revoke_key(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    state.revoke_key(&owner, &id)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn balance(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let agent = state.store.agent(&owner, &id)?;
    Ok(Json(
        state
            .bnb
            .balances(
                agent
                    .wallet
                    .as_deref()
                    .ok_or_else(|| ApiError::conflict("Register the agent first."))?,
            )
            .await?,
    ))
}
async fn purchase_quote(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<PurchaseQuote>> {
    state.store.agent(&owner, &id)?;
    Err(ApiError::unavailable(
        "A BNB Smart Chain USDT x402 provider is not connected yet.",
    ))
}
async fn purchase(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path((id, _quote_id)): Path<(String, String)>,
    Json(proof): Json<PurchaseSignature>,
) -> Result<Json<PurchaseResult>> {
    state.store.agent(&owner, &id)?;
    bnb::signature(&proof.signature)?;
    Err(ApiError::unavailable(
        "BNB Smart Chain USDT settlement is not enabled yet.",
    ))
}
async fn purchase_status(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path((id, _quote_id)): Path<(String, String)>,
) -> Result<Json<PurchaseResult>> {
    state.store.agent(&owner, &id)?;
    Err(ApiError::missing(
        "No BNB Smart Chain payment was submitted.",
    ))
}
async fn purchases(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Vec<PurchaseResult>>> {
    state.store.agent(&owner, &id)?;
    Ok(Json(vec![]))
}
#[derive(Deserialize, Default)]
struct ActivityQuery {
    after: Option<i64>,
    limit: Option<u64>,
}
async fn activity(
    State(state): State<AppState>,
    Query(query): Query<ActivityQuery>,
) -> Result<Json<Vec<AgentEvent>>> {
    let after = query.after.unwrap_or(0);
    let limit = query.limit.unwrap_or(100);
    if after < 0 || !(1..=500).contains(&limit) {
        return Err(ApiError::validation(
            "Use a nonnegative cursor and a limit from 1 to 500.",
        ));
    }
    Ok(Json(state.events(None, None, after, limit)?))
}
async fn live_agents(State(state): State<AppState>) -> Result<Json<Vec<PublicAgent>>> {
    Ok(Json(state.public_agents()?))
}
async fn records(State(state): State<AppState>) -> Result<Json<Vec<AgentRecord>>> {
    Ok(Json(state.public_records()?))
}
async fn live_agent(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>> {
    let agent = state
        .public_agents()?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| ApiError::missing("Public agent not found."))?;
    Ok(Json(
        json!({"agent":agent,"events":state.events(None,Some(&id),0,100)?,"runs":state.store.runs(&id)?.into_iter().map(|r|json!({"id":r.id,"status":r.status,"started_at":r.started_at,"finished_at":r.finished_at})).collect::<Vec<_>>()}),
    ))
}
async fn registry(State(state): State<AppState>) -> Json<RegistryData> {
    Json(state.registry().await)
}
async fn tools(State(state): State<AppState>) -> Json<Value> {
    let x402_ready = state
        .x402_system()
        .await
        .is_ok_and(|s| s["settlement_enabled"] == true);
    let funded = state.config.inference_daily_micros > 0;
    Json(
        json!({"bnb-rpc":true,"openrouter":funded && state.config.openrouter_key.is_some(),"tavily":funded && state.config.tavily_key.is_some(),"web-search":funded && (state.config.tavily_key.is_some() || state.config.openrouter_key.is_some()),"x402":x402_ready}),
    )
}
async fn capabilities(State(state): State<AppState>) -> Result<Json<Value>> {
    Ok(Json(state.provider_credit_status()?))
}
async fn models(State(state): State<AppState>) -> Json<Value> {
    Json(state.model_directory().await)
}
async fn feed(State(state): State<AppState>) -> Result<Json<Value>> {
    Ok(Json(state.feed()?))
}
async fn bounties(State(state): State<AppState>) -> Result<Json<Vec<Bounty>>> {
    Ok(Json(state.bounties(None)?))
}
async fn account_bounties(
    State(state): State<AppState>,
    Owner(owner): Owner,
) -> Result<Json<Vec<Bounty>>> {
    Ok(Json(state.bounties(Some(&owner))?))
}
async fn create_bounty(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Json(input): Json<BountyInput>,
) -> Result<(StatusCode, Json<Bounty>)> {
    Ok((
        StatusCode::CREATED,
        Json(state.create_bounty(&owner, input)?),
    ))
}
async fn metrics(State(state): State<AppState>) -> Result<Json<Value>> {
    Ok(Json(state.metrics().await?))
}
async fn token_system(State(state): State<AppState>) -> Json<Value> {
    Json(state.token_system().await)
}
async fn eligibility(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(state.eligibility(&owner, &id).await?))
}
async fn backing_assets(State(state): State<AppState>) -> Json<Value> {
    let mut assets = state.backing_assets();
    let deployed = state.bnb.deployed().await;
    if let Some(entries) = assets.as_array_mut() {
        for entry in entries {
            let supported = deployed && entry["network"] == state.config.network;
            entry["enabled"] = json!(supported);
            entry["status"] = json!(if supported {
                "custody_available"
            } else if entry["network"] != state.config.network {
                "different_network"
            } else {
                "awaiting_program_verification"
            });
        }
    }
    Json(assets)
}
async fn x402_system(State(state): State<AppState>) -> Result<Json<Value>> {
    Ok(Json(state.x402_system().await?))
}
#[derive(Deserialize)]
struct X402QuoteInput {
    merchant_id: String,
}
async fn x402_quote(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Json(input): Json<X402QuoteInput>,
) -> Result<Json<Value>> {
    Ok(Json(
        state.x402_quote(&owner, &id, &input.merchant_id).await?,
    ))
}
async fn financial_submitted(
    State(s): State<AppState>,
    Owner(o): Owner,
    Path(id): Path<String>,
    Json(p): Json<JobConfirmation>,
) -> Result<Json<Value>> {
    Ok(Json(s.submitted_action(&o, &id, &p.tx_hash, false).await?))
}
async fn job_submitted(
    State(s): State<AppState>,
    Owner(o): Owner,
    Path(id): Path<String>,
    Json(p): Json<JobConfirmation>,
) -> Result<Json<Value>> {
    Ok(Json(s.submitted_action(&o, &id, &p.tx_hash, true).await?))
}
async fn financial_release_failed(
    State(s): State<AppState>,
    Owner(o): Owner,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(s.release_failed_action(&o, &id, false).await?))
}
async fn job_release_failed(
    State(s): State<AppState>,
    Owner(o): Owner,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(s.release_failed_action(&o, &id, true).await?))
}
async fn credit_accounts(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(state.credit_accounts(&owner, &id).await?))
}
async fn finance_system(State(state): State<AppState>) -> Json<crate::finance::FinanceSystem> {
    Json(state.finance_system().await)
}
async fn finance_quote(
    State(state): State<AppState>,
    Json(input): Json<crate::finance::FinanceInput>,
) -> Result<Json<Value>> {
    Ok(Json(state.finance_quote(input).await?))
}
async fn finance_account(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(state.finance_account(&owner, &id).await?))
}
async fn finance_prepare(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Json(input): Json<crate::finance::FinanceInput>,
) -> Result<Json<TransactionIntent>> {
    Ok(Json(state.finance_prepare(&owner, &id, input).await?))
}
async fn finance_requests(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(state.finance_requests(&owner, &id).await?))
}
async fn finance_request(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Json(input): Json<crate::finance::FinanceInput>,
) -> Result<Json<crate::finance::FinanceRequest>> {
    Ok(Json(state.finance_request(&owner, &id, input).await?))
}
async fn wallet_actions(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Vec<TransactionIntent>>> {
    Ok(Json(state.wallet_actions(&owner, &id)?))
}
async fn financial_prepare(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Json(input): Json<FinancialInput>,
) -> Result<Json<TransactionIntent>> {
    Ok(Json(state.financial_prepare(&owner, &id, input).await?))
}
async fn financial_confirm(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Json(input): Json<JobConfirmation>,
) -> Result<Json<Value>> {
    Ok(Json(
        state.financial_confirm(&owner, &id, &input.tx_hash).await?,
    ))
}
async fn job_system(State(state): State<AppState>) -> Json<JobSystem> {
    Json(state.job_system().await)
}
async fn job_services(State(state): State<AppState>) -> Result<Json<Vec<JobMerchant>>> {
    Ok(Json(state.merchants()?))
}
async fn public_jobs(State(state): State<AppState>) -> Result<Json<Vec<PublicJob>>> {
    Ok(Json(state.public_jobs()?))
}
#[derive(Deserialize, Default)]
struct JobsQuery {
    agent_id: Option<String>,
}
async fn account_jobs(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Query(query): Query<JobsQuery>,
) -> Result<Json<Vec<Job>>> {
    Ok(Json(state.list_jobs(
        &owner,
        query.agent_id.as_deref(),
        false,
    )?))
}
async fn job_actions(
    State(state): State<AppState>,
    Owner(owner): Owner,
) -> Result<Json<Vec<JobActionRecord>>> {
    Ok(Json(state.job_actions(&owner)?))
}
async fn account_job(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Job>> {
    Ok(Json(state.get_job(&owner, &id)?))
}
async fn create_job(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Json(input): Json<JobInput>,
) -> Result<(StatusCode, Json<Job>)> {
    Ok((
        StatusCode::CREATED,
        Json(state.create_job(&owner, &id, input, None).await?),
    ))
}
async fn create_branch(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Json(input): Json<JobInput>,
) -> Result<(StatusCode, Json<Job>)> {
    let parent = state.get_job(&owner, &id)?;
    Ok((
        StatusCode::CREATED,
        Json(
            state
                .create_job(&owner, &parent.plan.executor_id, input, Some(&id))
                .await?,
        ),
    ))
}
async fn collect_evidence(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Job>> {
    Ok(Json(state.collect_evidence(&owner, &id).await?))
}
async fn attach_evidence(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Json(evidence): Json<Value>,
) -> Result<Json<Job>> {
    Ok(Json(state.attach_evidence(&owner, &id, evidence)?))
}
async fn cancel_draft(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Job>> {
    Ok(Json(state.cancel_draft(&owner, &id)?))
}
async fn prepare_job(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Json(input): Json<JobAction>,
) -> Result<Json<JobIntent>> {
    if let Some(invoice) = input.invoice {
        if input.action != "pay" {
            return Err(ApiError::validation(
                "A merchant invoice belongs to the pay action.",
            ));
        }
        return Ok(Json(state.prepare_invoice(&owner, &id, invoice).await?));
    }
    Ok(Json(state.prepare_job(&owner, &id, &input.action).await?))
}
async fn confirm_job(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Json(proof): Json<JobConfirmation>,
) -> Result<Json<Job>> {
    Ok(Json(state.confirm_job(&owner, &id, &proof.tx_hash).await?))
}
async fn refresh_job(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Job>> {
    Ok(Json(state.refresh_job(&owner, &id, None).await?))
}
async fn builder_jobs(
    State(state): State<AppState>,
    Builder(owner, id): Builder,
) -> Result<Json<Vec<Job>>> {
    Ok(Json(state.list_jobs(&owner, Some(&id), true)?))
}
async fn builder_branch(
    State(state): State<AppState>,
    Builder(owner, agent_id): Builder,
    Path(id): Path<String>,
    Json(input): Json<JobInput>,
) -> Result<(StatusCode, Json<Job>)> {
    let parent = state.get_job(&owner, &id)?;
    if parent.plan.executor_id != agent_id {
        return Err(ApiError::forbidden(
            "This access key is not the assigned executor.",
        ));
    }
    Ok((
        StatusCode::CREATED,
        Json(
            state
                .create_job(&owner, &agent_id, input, Some(&id))
                .await?,
        ),
    ))
}
async fn builder_evidence(
    State(state): State<AppState>,
    Builder(owner, agent_id): Builder,
    Path(id): Path<String>,
) -> Result<Json<Job>> {
    let job = state.get_job(&owner, &id)?;
    if job.plan.executor_id != agent_id {
        return Err(ApiError::forbidden(
            "This access key is not the assigned executor.",
        ));
    }
    Ok(Json(state.collect_evidence(&owner, &id).await?))
}
async fn transaction_status(
    State(state): State<AppState>,
    Path(signature): Path<String>,
) -> Result<Json<Value>> {
    let mut status = state.bnb.transaction_status(&signature).await?;
    if status["receipt"].is_object() {
        let transaction = state
            .bnb
            .rpc("eth_getTransactionByHash", json!([signature]))
            .await?;
        if transaction["hash"]
            .as_str()
            .is_none_or(|hash| !hash.eq_ignore_ascii_case(&signature))
            || transaction["chainId"] != "0x38"
            || transaction["blockHash"] != status["receipt"]["blockHash"]
            || transaction["blockNumber"] != status["receipt"]["blockNumber"]
        {
            return Err(ApiError::unavailable(
                "The transaction envelope does not match its canonical receipt.",
            ));
        }
        status["transaction"] = transaction;
    }
    Ok(Json(status))
}
async fn token_metadata(
    State(state): State<AppState>,
    Path(address): Path<String>,
) -> Result<Json<TokenMetadata>> {
    let address = crate::bnb::address(&address)?;
    state.bnb.require_network().await?;
    let code = state
        .bnb
        .rpc("eth_getCode", json!([address, "latest"]))
        .await?;
    if !code
        .as_str()
        .is_some_and(|s| s.starts_with("0x") && s.len() > 2)
    {
        return Err(ApiError::validation(
            "No token contract exists at this BNB address.",
        ));
    }
    let decimals = state.bnb.token_decimals(&address).await?;
    Ok(Json(TokenMetadata {
        chain_id: 56,
        address,
        decimals,
    }))
}
async fn openapi() -> Json<Value> {
    Json(crate::schema::document())
}
async fn docs() -> Html<&'static str> {
    Html("<!doctype html><html lang=en><meta charset=utf-8><title>Tab Rust API</title><body><h1>Tab API</h1><p>BNB Smart Chain agents, USDT tasks and verified action receipts.</p><p><a href=/api/openapi.json>OpenAPI specification</a></p><p>Account routes require a Privy ES256 bearer token. Agent routes require a scoped agent access key. Financial requests remain disabled until the BNB Smart Chain deployment and token configuration are verified.</p></body></html>")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct X402ExecuteInput {
    signature: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct X402ReconcileInput {
    tx_hash: String,
}
async fn x402_execute(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path((id, quote_id)): Path<(String, String)>,
    Json(input): Json<X402ExecuteInput>,
) -> Result<Json<Value>> {
    Ok(Json(
        state
            .x402_execute(&owner, &id, &quote_id, &input.signature)
            .await?,
    ))
}
async fn x402_reconcile(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path((id, quote_id)): Path<(String, String)>,
    Json(input): Json<X402ReconcileInput>,
) -> Result<Json<Value>> {
    Ok(Json(
        state
            .x402_reconcile(&owner, &id, &quote_id, &input.tx_hash)
            .await?,
    ))
}

async fn x402_history(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(state.x402_history(&owner, &id)?))
}

async fn x402_release_expired(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path((id, quote_id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    Ok(Json(
        state.x402_release_expired(&owner, &id, &quote_id).await?,
    ))
}
