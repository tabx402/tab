use crate::{api, bnb, config::Config, db::encode, models::*, AppState};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use rusqlite::params;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use tower::ServiceExt;

pub(crate) fn state() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let config = Config {
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
        database: root.join("test.sqlite"),
        manifest: root.join("missing.json"),
        merchants: root.join("merchants.json"),
        x402_merchants: root.join("x402.json"),
        bind: "127.0.0.1:0".into(),
        openrouter_key: None,
        tavily_key: None,
        inference_daily_micros: 100000,
    };
    let app = AppState::new(config).unwrap();
    (dir, app)
}
fn agent_input(public: bool) -> AgentInput {
    serde_json::from_value(json!({"name":"willow","purpose":"research the published protocol docs","tools":["bnb-rpc","openrouter"],"daily_cap":"10","max_call":"0.1","public_activity":public})).unwrap()
}
#[test]
fn retried_creation_has_one_agent_and_rejects_changed_payload() {
    let (_dir, app) = state();
    let key = "9dfb9a7b-e87f-4cc4-af29-a953e2c93e05";
    let first = app
        .create_agent_once("owner-a", agent_input(true), Some(key))
        .unwrap();
    let second = app
        .create_agent_once("owner-a", agent_input(true), Some(key))
        .unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(app.store.list_agents("owner-a").unwrap().len(), 1);
    let mut changed = agent_input(true);
    changed.name = "different agent".into();
    assert!(app
        .create_agent_once("owner-a", changed, Some(key))
        .is_err());
    let other = app
        .create_agent_once("owner-b", agent_input(true), Some(key))
        .unwrap();
    assert_ne!(other.id, first.id);
    assert!(app.account_profile("owner-a").unwrap()["registered"]
        .as_bool()
        .unwrap());
    assert_eq!(app.account_profile("owner-b").unwrap()["agents"], 1);
}
#[test]
fn account_profile_is_private_and_registration_is_idempotent() {
    let (_dir, app) = state();
    assert_eq!(app.account_profile("first").unwrap()["registered"], false);
    let a = app
        .register_account(
            "first",
            AccountInput {
                display_name: "Ryan".into(),
            },
        )
        .unwrap();
    let b = app
        .register_account(
            "first",
            AccountInput {
                display_name: String::new(),
            },
        )
        .unwrap();
    assert_eq!(a["created_at"], b["created_at"]);
    assert_eq!(b["display_name"], "Ryan");
    assert_eq!(app.account_profile("second").unwrap()["registered"], false);
}
fn job_input(executor: &str, budget: &str) -> JobInput {
    serde_json::from_value(json!({"title":"review source documents","description":"compare primary sources and provide links","executor_id":executor,"budget":budget,"max_call":"0.1","deadline":(Utc::now()+Duration::hours(4)).to_rfc3339(),"tools":["bnb-rpc"],"public_activity":true})).unwrap()
}
async fn get(app: axum::Router, path: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn public_api_is_bnb_and_fail_closed_without_keys_program_or_tab() {
    let (_dir, state) = state();
    let app = api::router(state.clone());
    let (status, health) = get(app.clone(), "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["backend"], "rust");
    let (_, config) = get(app.clone(), "/api/config").await;
    assert_eq!(config["chain_id"], 56);
    assert_eq!(config["preferred_wallet"], "metamask");
    assert_eq!(config["financial_actions_enabled"], false);
    assert_eq!(config["official_tab_address"], Value::Null);
    let (_, feed) = get(app.clone(), "/api/feed").await;
    assert_eq!(feed["items"], json!([]));
    assert_eq!(feed["agents"], json!([]));
    let (_, metrics) = get(app.clone(), "/api/metrics").await;
    assert_eq!(metrics["customer_payments_usdt"], "0");
    assert_eq!(metrics["buyback_usdt_spent"], "0");
    let (_, token) = get(app.clone(), "/api/token/system").await;
    assert_eq!(token["staking_enabled"], false);
    assert_eq!(token["buyer_disagreement_slashable"], false);
    let (_, x402) = get(app, "/api/x402/system").await;
    assert_eq!(x402["settlement_enabled"], false);
}
#[tokio::test]
async fn retired_overview_never_serves_generated_activity_or_changes_records() {
    let (_dir, state) = state();
    let agent = state.create_agent("owner", agent_input(true)).unwrap();
    let events_before = state.events(Some("owner"), None, 0, 100).unwrap();
    let app = api::router(state.clone());
    for path in [
        "/api/overview",
        "/api/overview?mode=public",
        "/api/overview?mode=example",
        "/api/overview?mode=demo",
    ] {
        let (status, body) = get(app.clone(), path).await;
        assert_eq!(status, StatusCode::GONE);
        assert!(body["detail"].as_str().unwrap().contains("actual records"));
        assert!(body.get("agents").is_none());
        assert!(body.get("receipts").is_none());
    }
    assert_eq!(state.store.list_agents("owner").unwrap().len(), 1);
    assert_eq!(
        state.store.agent("owner", &agent.id).unwrap().plan.name,
        "willow"
    );
    assert_eq!(state.store.runs(&agent.id).unwrap().len(), 0);
    assert_eq!(
        state.events(Some("owner"), None, 0, 100).unwrap().len(),
        events_before.len()
    );
    let (_, live) = get(app.clone(), "/api/agents/live").await;
    assert_eq!(live, json!([]));
    let (_, records) = get(app, "/api/agents/records").await;
    assert_eq!(records, json!([]));
}
#[tokio::test]
async fn private_routes_reject_unverified_access_and_disable_caching() {
    let (_dir, state) = state();
    let response = api::router(state)
        .oneshot(
            Request::builder()
                .uri("/api/account/runtime")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["cache-control"], "no-store");
}
#[test]
fn account_isolation_and_bnb_public_key_validation() {
    let (_dir, state) = state();
    let agent = state
        .create_agent("did:privy:a", agent_input(false))
        .unwrap();
    assert!(state.store.agent("did:privy:b", &agent.id).is_err());
    assert!(state.store.list_agents("did:privy:b").unwrap().is_empty());
    let mut invalid = agent_input(true);
    invalid.watch_address = Some("not-an-evm-address".into());
    assert!(invalid.validate().is_err());
    assert!(bnb::pubkey("0x1234").is_err());
    assert!(bnb::signature("not-a-transaction").is_err());
}
#[test]
fn multiple_models_and_numeric_inputs_are_supported_but_policy_bounds_hold() {
    for model in MODELS {
        let mut plan = agent_input(true);
        plan.model = (*model).into();
        assert!(plan.validate().is_ok());
    }
    let mut value = serde_json::to_value(agent_input(true)).unwrap();
    value["max_call"] = json!(0.1);
    assert!(serde_json::from_value::<AgentInput>(value).is_err());
    let mut invalid = agent_input(true);
    invalid.tools.push("bnb-rpc".into());
    assert!(invalid.validate().is_err());
    let mut invalid = agent_input(true);
    invalid.max_call = Decimal::new(1, 19);
    assert!(invalid.validate().is_err());
    assert!(units(Decimal::MAX).is_err());
    assert_eq!(tool_bitmap(&["bnb-rpc".into(), "x402".into()]), 17);
    assert_eq!(tool_bitmap(&["openrouter".into(), "tavily".into()]), 6);
}
#[test]
fn per_call_cap_cannot_exceed_daily_cap_on_creation_or_update() {
    let (_dir, state) = state();
    let mut plan = agent_input(true);
    plan.daily_cap = Decimal::ONE;
    plan.max_call = Decimal::new(1_000_001, 6);
    assert_eq!(
        state.create_agent("owner", plan.clone()).err().unwrap().0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(state.store.list_agents("owner").unwrap().is_empty());

    plan.max_call = Decimal::ONE;
    let agent = state.create_agent("owner", plan.clone()).unwrap();
    plan.max_call = Decimal::new(1_000_001, 6);
    assert_eq!(
        state
            .update_agent("owner", &agent.id, plan.clone())
            .err()
            .unwrap()
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        state.store.agent("owner", &agent.id).unwrap().plan.max_call,
        Decimal::ONE
    );

    plan.max_call = Decimal::new(1, 6);
    assert_eq!(
        state
            .update_agent("owner", &agent.id, plan)
            .unwrap()
            .plan
            .max_call,
        Decimal::new(1, 6)
    );
    for (daily_cap, max_call) in [(5, "5"), (10, "10"), (25, "10")] {
        let mut valid = agent_input(true);
        valid.daily_cap = Decimal::from(daily_cap);
        valid.max_call = max_call.parse().unwrap();
        assert!(valid.validate().is_ok());
    }
}

#[tokio::test]
async fn tasks_save_as_unfunded_drafts_before_wallet_registration() {
    let (_dir, state) = state();
    let a = state
        .create_agent("did:privy:a", agent_input(true))
        .unwrap();
    let b = state
        .create_agent("did:privy:a", agent_input(true))
        .unwrap();
    let job = state
        .create_job("did:privy:a", &a.id, job_input(&b.id, "4"), None)
        .await
        .unwrap();
    assert_eq!(job.funding, "unfunded");
    assert!(job.buyer_wallet.is_empty());
    assert!(job.executor_wallet.is_empty());
    assert!(state
        .prepare_job("did:privy:a", &job.id, "fund")
        .await
        .is_err());
    assert_eq!(
        state.get_job("did:privy:a", &job.id).unwrap().state,
        "draft"
    );
}
#[tokio::test]
async fn branches_reserve_once_and_cannot_reuse_the_parent_budget() {
    let (_dir, state) = state();
    let a = state.create_agent("a", agent_input(true)).unwrap();
    let b = state.create_agent("a", agent_input(true)).unwrap();
    let c = state.create_agent("a", agent_input(true)).unwrap();
    let root = state
        .create_job("a", &a.id, job_input(&b.id, "4"), None)
        .await
        .unwrap();
    let mut branch = job_input(&c.id, "3");
    branch.deadline = root.plan.deadline;
    let child = state
        .create_job("a", &b.id, branch, Some(&root.id))
        .await
        .unwrap();
    assert_eq!(
        state.get_job("a", &root.id).unwrap().available,
        Decimal::ONE
    );
    assert!(state
        .create_job("a", &b.id, job_input(&c.id, "2"), Some(&root.id))
        .await
        .is_err());
    state.cancel_draft("a", &child.id).unwrap();
    assert_eq!(
        state.get_job("a", &root.id).unwrap().available,
        Decimal::from(4)
    );
    assert!(state.cancel_draft("a", &child.id).is_err());
    assert_eq!(
        state.get_job("a", &root.id).unwrap().available,
        Decimal::from(4)
    );
}
#[tokio::test]
async fn delegation_narrows_caps_and_full_tree_privacy() {
    let (_dir, state) = state();
    let a = state.create_agent("a", agent_input(true)).unwrap();
    let b = state.create_agent("a", agent_input(false)).unwrap();
    let c = state.create_agent("b", agent_input(true)).unwrap();
    let root = state
        .create_job("a", &a.id, job_input(&b.id, "4"), None)
        .await
        .unwrap();
    assert!(state.public_jobs().unwrap().is_empty());
    let mut wide = job_input(&c.id, "1");
    wide.max_call = Decimal::new(2, 1);
    assert!(state
        .create_job("a", &b.id, wide, Some(&root.id))
        .await
        .is_err());
    assert!(state.get_job("b", &root.id).is_err());
}
#[tokio::test]
async fn public_feed_omits_private_agents_and_never_invents_receipts() {
    let (_dir, state) = state();
    let private = state.create_agent("a", agent_input(false)).unwrap();
    let public = state.create_agent("a", agent_input(true)).unwrap();
    state
        .event(
            &private,
            "run_completed",
            "private details",
            "completed",
            None,
            None,
            None,
            None,
        )
        .unwrap();
    state
        .event(
            &public,
            "run_completed",
            "public receipt",
            "completed",
            None,
            None,
            None,
            None,
        )
        .unwrap();
    let feed = state.feed().unwrap();
    assert_eq!(feed["items"].as_array().unwrap().len(), 1);
    assert_eq!(feed["items"][0]["agent_id"], public.id);
    assert_eq!(state.events(Some("a"), None, 0, 100).unwrap().len(), 2);
}
#[test]
fn replayed_partial_runs_do_not_inflate_completed_records() {
    let (_dir, state) = state();
    let public = state.create_agent("dev-test", agent_input(true)).unwrap();
    let private = state.create_agent("dev-test", agent_input(false)).unwrap();
    for (agent, kind, status) in [
        (&public, "run_completed", "completed"),
        // Previously saved partial runs keep their original event kind.
        (&public, "run_completed", "partial"),
        (&public, "run_partial", "partial"),
        (&public, "run_failed", "failed"),
        (&private, "run_completed", "completed"),
    ] {
        state
            .event(
                agent,
                kind,
                "local regression replay",
                status,
                None,
                None,
                None,
                None,
            )
            .unwrap();
    }
    let records = state.public_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, public.id);
    assert_eq!(records[0].runs, 1);
    assert_eq!(records[0].failures, 1);
    assert_eq!(records[0].payments, 0);
    assert_eq!(records[0].paid, Decimal::ZERO);
    assert_eq!(state.events(None, None, 0, 100).unwrap().len(), 4);
    assert_eq!(
        crate::runtime::run_completion("completed").0,
        "run_completed"
    );
    assert_eq!(crate::runtime::run_completion("partial").0, "run_partial");
    assert_eq!(crate::runtime::run_completion("failed").0, "run_failed");
}

#[test]
fn buyer_disagreement_is_not_an_objective_bond_penalty() {
    let mut plan = job_input(&"a".repeat(32), "1");
    plan.bond_tokens = Decimal::ONE;
    plan.bond_token_address = Some(bnb::USDT.into());
    plan.penalty_rule = Some("buyer_disliked_report".into());
    assert!(plan.clone().validate().is_err());
    plan.penalty_rule = Some("deadline_missed".into());
    assert!(plan.validate().is_ok());
}

#[tokio::test]
async fn an_account_plan_never_creates_wallet_financial_authority() {
    let (_dir, state) = state();
    let agent = state.create_agent("a", agent_input(true)).unwrap();
    let input: FinancialInput =
        serde_json::from_value(json!({"action":"stake","amount":"1"})).unwrap();
    assert!(state
        .financial_prepare("a", &agent.id, input)
        .await
        .is_err());
    assert!(state.create_key("a", &agent.id).is_err());
}
#[test]
fn schema_contains_rust_bnb_contracts_and_freeform_output_objects() {
    let schema = crate::schema::document();
    assert_eq!(schema["info"]["version"], "0.2.0");
    assert!(schema["paths"]["/api/account/runtime/{id}/wallet-actions/prepare"].is_object());
    assert_eq!(
        schema["components"]["schemas"]["AgentRun"]["properties"]["output"]["type"],
        "object"
    );
    let serialized = serde_json::to_string(&schema).unwrap().to_lowercase();
    assert!(serialized.contains("bnb"));
    assert!(serialized.contains("usdt"));
    assert!(schema["paths"].get("/api/overview").is_none());
    for retired in ["AgentExample", "PublicData", "Receipt", "Summary"] {
        assert!(schema["components"]["schemas"].get(retired).is_none());
    }
}
#[test]
fn database_roundtrip_preserves_exact_decimal_policies() {
    let (_dir, state) = state();
    let agent = state.create_agent("a", agent_input(false)).unwrap();
    let saved = state.store.agent("a", &agent.id).unwrap();
    assert_eq!(encode(&saved).unwrap(), encode(&agent).unwrap());
    state
        .store
        .connect()
        .unwrap()
        .execute(
            "INSERT INTO agent_runs VALUES(?,?,?,?,?,?)",
            params!["run", agent.id, "completed", now(), now(), "{}"],
        )
        .unwrap();
    assert_eq!(state.store.runs(&agent.id).unwrap().len(), 1);
}
#[tokio::test]
async fn owner_revoke_invalidates_the_builder_key_and_other_accounts_cannot_revoke() {
    let (_dir, state) = state();
    let mut agent = state.create_agent("a", agent_input(true)).unwrap();
    agent.wallet = Some(bnb::ZERO.into());
    state.store.save_agent(&agent).unwrap();
    let key = state.create_key("a", &agent.id).unwrap();
    assert!(state.revoke_key("b", &agent.id).is_err());
    let request = Request::builder()
        .uri("/api/agent/jobs")
        .header("Authorization", format!("Bearer {key}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        api::router(state.clone())
            .oneshot(request)
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    state.revoke_key("a", &agent.id).unwrap();
    let request = Request::builder()
        .uri("/api/agent/jobs")
        .header("Authorization", format!("Bearer {key}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        api::router(state).oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn expired_unsigned_intent_unblocks_drafts_but_submitted_intent_stays_reserved() {
    let (_dir, state) = state();
    let buyer = state.create_agent("a", agent_input(true)).unwrap();
    let executor = state.create_agent("a", agent_input(true)).unwrap();
    let root = state
        .create_job("a", &buyer.id, job_input(&executor.id, "4"), None)
        .await
        .unwrap();
    let intent = JobIntent {
        tx_hash: None,
        id: identifier(),
        job_id: root.id.clone(),
        action: "fund".into(),
        chain_id: 56,
        network: "mainnet".into(),
        sender: bnb::ZERO.into(),
        to: bnb::ZERO.into(),
        data: "expired unsigned bytes".into(),
        value: "0".into(),
        approval: None,
        transactions: vec![],
        expires_at: (Utc::now() - Duration::minutes(5)).to_rfc3339(),
    };
    state
        .store
        .connect()
        .unwrap()
        .execute(
            "INSERT INTO job_intents(id,job_id,owner,payload) VALUES(?,?,?,?)",
            params![intent.id, root.id, "a", encode(&intent).unwrap()],
        )
        .unwrap();
    let mut plan = job_input(&executor.id, "1");
    plan.deadline = root.plan.deadline;
    assert!(state
        .create_job("a", &executor.id, plan.clone(), Some(&root.id))
        .await
        .is_ok());
    state
        .store
        .connect()
        .unwrap()
        .execute(
            "UPDATE job_intents SET tx_hash=? WHERE id=?",
            params![format!("0x{}", "11".repeat(32)), intent.id],
        )
        .unwrap();
    assert!(state
        .create_job("a", &executor.id, plan, Some(&root.id))
        .await
        .is_err());
}
#[test]
fn delegated_session_nonce_roundtrips_without_javascript_integer_rounding() {
    let input: FinancialInput = serde_json::from_value(
        json!({"action":"revoke_session","session_nonce":u64::MAX.to_string()}),
    )
    .unwrap();
    assert_eq!(input.session_nonce, Some(u64::MAX));
    assert_eq!(
        serde_json::to_value(&input).unwrap()["session_nonce"],
        u64::MAX.to_string()
    );
}

#[tokio::test]
async fn configured_keys_with_zero_provider_budget_are_not_advertised_as_available() {
    let (_dir, state) = state();
    let mut config = (*state.config).clone();
    config.openrouter_key = Some("unused-test-key".into());
    config.tavily_key = Some("unused-test-key".into());
    config.inference_daily_micros = 0;
    let state = AppState::new(config).unwrap();
    let (_, tools) = get(api::router(state.clone()), "/api/tools").await;
    assert_eq!(tools["bnb-rpc"], true);
    for tool in ["openrouter", "tavily", "web-search", "x402"] {
        assert_eq!(tools[tool], false);
    }
    for provider in state.providers().await {
        if ["openrouter", "tavily", "x402"].contains(&provider.id.as_str()) {
            assert_ne!(provider.status, "live");
        }
    }
}

#[test]
fn event_run_migration_preserves_legacy_rows_without_guessing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE network_identity(chain TEXT PRIMARY KEY); INSERT INTO network_identity VALUES('eip155:56:usdt18'); CREATE TABLE agent_events(id INTEGER PRIMARY KEY AUTOINCREMENT,agent_id TEXT NOT NULL,kind TEXT NOT NULL,status TEXT NOT NULL,at TEXT NOT NULL,message TEXT NOT NULL,provider TEXT,amount TEXT,currency TEXT,tx_hash TEXT); INSERT INTO agent_events(agent_id,kind,status,at,message) VALUES('agent','run_completed','completed','same-time','legacy');").unwrap();
    drop(db);
    for _ in 0..2 {
        let store = crate::db::Store::open(&path).unwrap();
        let row: (String, Option<String>) = store.connect().unwrap().query_row("SELECT message,run_id FROM agent_events WHERE id=1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(row, ("legacy".into(), None));
    }
}

#[test]
fn event_previews_and_receipts_use_exact_run_ids_and_preserve_privacy() {
    let (_dir, state) = state();
    let mut public = state.create_agent("a", agent_input(true)).unwrap();
    public.registry_id = Some("public-registry-id".into());
    state.store.save_agent(&public).unwrap();
    let private = state.create_agent("b", agent_input(false)).unwrap();
    let unlisted = state.create_agent("c", agent_input(true)).unwrap();
    let db = state.store.connect().unwrap();
    db.execute("INSERT INTO agent_public_exclusions VALUES(?,?)", params![unlisted.id,"same-time"]).unwrap();
    for (id, agent, summary) in [("one", &public, "first result"), ("two", &public, "second result"), ("private", &private, "private result"), ("unlisted", &unlisted, "unlisted result")] {
        db.execute("INSERT INTO agent_runs VALUES(?,?,?,?,?,?)",params![id,agent.id,"completed","same-time","same-time",json!({"openrouter":{"summary":summary,"prompt":"hidden prompt"}}).to_string()]).unwrap();
        state.event_for_run(agent,"run_completed","finished","completed",None,None,None,None,Some(id)).unwrap();
    }
    for run in [None, Some("missing"), Some("private")] {
        state.event_for_run(&public,"model_result","uncorrelated","completed",None,None,None,None,run).unwrap();
    }
    let events = state.events(None,None,0,100).unwrap();
    assert!(events.iter().all(|e| e.agent_id == public.id));
    for event in &events {
        match event.run_id.as_deref() {
            Some("one") => assert_eq!(event.preview.as_ref().unwrap()["summary"], "first result"),
            Some("two") => assert_eq!(event.preview.as_ref().unwrap()["summary"], "second result"),
            _ => assert!(event.preview.is_none()),
        }
    }
    let receipt = state.public_run_receipt(&public.id,"one").unwrap();
    assert_eq!(receipt["events"].as_array().unwrap().len(),1);
    assert_eq!(receipt["events"][0]["run_id"],"one");
    assert!(!receipt.to_string().contains("hidden prompt"));
    assert_eq!(state.events(Some("b"),None,0,100).unwrap().len(),1);
}
