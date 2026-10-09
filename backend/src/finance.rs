//! Optional finance modules. A source build or an address is never funding or deployment proof.
use crate::{
    bnb::{self, Instruction},
    db::{decode, encode, Store},
    error::{ApiError, Result},
    models::*,
    AppState,
};
use chrono::{Duration, Utc};
use ethabi::{Contract, Token};
use rusqlite::{params, OptionalExtension};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, io::Cursor, path::PathBuf};

#[derive(Clone, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FinanceInput {
    pub action: String,
    #[serde(default, with = "rust_decimal::serde::str")]
    #[schema(value_type=String)]
    pub amount: Decimal,
    #[serde(default)]
    pub loan_id: Option<String>,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub target_agent_id: Option<String>,
    #[serde(default)]
    pub token_address: Option<String>,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[schema(value_type=Option<String>)]
    pub collateral_amount: Option<Decimal>,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[schema(value_type=Option<String>)]
    pub per_call: Option<Decimal>,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[schema(value_type=Option<String>)]
    pub daily_cap: Option<Decimal>,
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub signer: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub recipients: Vec<String>,
    #[serde(default)]
    pub recipient: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub request_hash: Option<String>,
    #[serde(default)]
    pub receipt_hash: Option<String>,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[schema(value_type=Option<String>)]
    pub minimum_out: Option<Decimal>,
    #[serde(default)]
    pub deadline: Option<i64>,
    #[serde(default, rename = "public")]
    pub public_activity: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    const WALLET: &str = "0x4444444444444444444444444444444444444444";
    const LENDING: &str = "0x7777777777777777777777777777777777777777";
    const PROTOCOL: &str = "0x1111111111111111111111111111111111111111";
    fn encoded(tokens: &[Token]) -> Value {
        json!(format!("0x{}", hex::encode(ethabi::encode(tokens))))
    }
    fn call(signature: &str) -> String {
        format!("call:0x{}", &bnb::hash(signature.as_bytes())[2..10])
    }
    fn input(action: &str, amount: &str) -> FinanceInput {
        serde_json::from_value(json!({"action":action,"amount":amount})).unwrap()
    }
    fn create_agent(state: &AppState, ready: bool, paused: bool) -> RuntimeAgent {
        let mut agent=state.create_agent("owner",serde_json::from_value(json!({"name":"willow","purpose":"monitor a public wallet","daily_cap":"10","max_call":"0.1","tools":["bnb-rpc"],"public_activity":false})).unwrap()).unwrap();
        if ready {
            agent.wallet = Some(WALLET.into());
            agent.registry_id = Some(state.bnb.agent_address(WALLET, &agent.id).unwrap());
            agent.status = if paused { "paused" } else { "ready" }.into();
            state.store.save_agent(&agent).unwrap();
        }
        agent
    }
    fn enable_lending(f: &crate::chain_tests::Fixture, paused: bool) {
        let hash = bnb::hash(&hex::decode("60006000").unwrap());
        let mut config: Value =
            serde_json::from_str(include_str!("../config/finance-bnb.json")).unwrap();
        config["modules"]["lending"] = json!({"address":LENDING,"code_hash":hash});
        std::fs::write(
            f.state.finance_config_path(),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let mut replies = f.replies.lock().unwrap();
        replies.insert(
            call("getProtocol()"),
            encoded(&[Token::Tuple(vec![
                bnb::addr(WALLET).unwrap(),
                bnb::addr(bnb::USDT).unwrap(),
                bnb::addr(bnb::ZERO).unwrap(),
                bnb::addr("0x2222222222222222222222222222222222222222").unwrap(),
                bnb::addr("0x3333333333333333333333333333333333333333").unwrap(),
                bnb::uint(200),
                bnb::uint(0),
                bnb::uint(0),
                bnb::uint(0),
                bnb::uint(0),
            ])]),
        );
        replies.insert(call("authority()"), encoded(&[bnb::addr(WALLET).unwrap()]));
        replies.insert(
            call("getAgent(bytes32)"),
            encoded(&[Token::Tuple(vec![
                bnb::addr(WALLET).unwrap(),
                Token::String("willow".into()),
                bnb::uint(10_000_000_000_000_000_000),
                Token::FixedBytes(vec![1; 32]),
                Token::Bool(paused),
                bnb::uint(1),
                bnb::uint(0),
                bnb::uint(0),
            ])]),
        );
        replies.insert(
            call("getPool()"),
            encoded(&[Token::Tuple(vec![
                bnb::addr(bnb::USDT).unwrap(),
                bnb::addr(WALLET).unwrap(),
                Token::Bool(paused),
                bnb::uint(100_000_000_000_000_000_000),
                bnb::uint(0),
                bnb::uint(0),
                bnb::uint(100_000_000_000_000_000_000),
                bnb::uint(100_000_000_000_000_000_000),
            ])]),
        );
        for signature in [
            "balanceOf(address)",
            "maxRedeem(address)",
            "maxDeposit(address)",
            "previewRedeem(uint256)",
        ] {
            replies.insert(
                call(signature),
                encoded(&[bnb::uint(100_000_000_000_000_000_000)]),
            );
        }
    }
    #[test]
    fn finance_inputs_reject_user_calldata_and_inexact_or_zero_amounts() {
        assert!(serde_json::from_value::<FinanceInput>(
            json!({"action":"pool_deposit","amount":"5","calldata":"0x"})
        )
        .is_err());
        assert!(serde_json::from_value::<FinanceInput>(
            json!({"action":"pool_deposit","amount":5})
        )
        .is_err());
        for (amount, decimals) in [
            ("0", 18),
            ("-1", 18),
            ("0.0000001", 6),
            ("0.0000000000000000001", 18),
        ] {
            assert!(positive(amount.parse().unwrap(), decimals).is_err());
        }
        assert_eq!(positive("1.000001".parse().unwrap(), 6).unwrap(), 1_000_001);
        assert!(!recovery("stock_borrow"));
        assert!(recovery("stock_repay"));
        assert!(recovery("stock_withdraw"));
    }
    #[tokio::test]
    async fn missing_optional_deployments_are_explicit_and_do_not_create_financial_requests() {
        let f = crate::chain_tests::fixture().await;
        let agent = create_agent(&f.state, false, false);
        let system = f.state.finance_system().await;
        assert_eq!(system.source, "deployment_pending");
        for module in [
            &system.modules.lending,
            &system.modules.stock_loans,
            &system.modules.buyback,
        ] {
            assert_eq!(module.status, "not_deployed");
            assert!(module.address.is_none());
            assert!(module.available_usdt.is_none());
        }
        assert!(f
            .state
            .finance_request("owner", &agent.id, input("advance_request", "5"))
            .await
            .is_err());
        let account = f.state.finance_account("owner", &agent.id).await.unwrap();
        assert_eq!(account["requests"], json!([]));
        assert_eq!(account["roles"]["underwriter"], false);
        assert!(f
            .replies
            .lock()
            .unwrap()
            .get("request:eth_chainId")
            .is_none());
    }
    #[tokio::test]
    async fn wrong_chain_and_wrong_module_hash_cannot_enable_finance() {
        let f = crate::chain_tests::fixture().await;
        enable_lending(&f, false);
        let system = f.state.finance_system().await;
        assert_eq!(system.modules.lending.status, "verified");
        assert_eq!(
            system.modules.lending.available_usdt.as_deref(),
            Some("100")
        );
        f.replies
            .lock()
            .unwrap()
            .insert(format!("code:{LENDING}"), json!("0x6001"));
        assert_eq!(
            f.state.finance_system().await.modules.lending.status,
            "unavailable"
        );
        let mut config: Value =
            serde_json::from_slice(&std::fs::read(f.state.finance_config_path()).unwrap()).unwrap();
        config["chain_id"] = json!(1);
        std::fs::write(
            f.state.finance_config_path(),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        assert_eq!(
            f.state.finance_system().await.modules.lending.status,
            "unavailable"
        );
    }
    #[tokio::test]
    async fn deposit_binds_exact_approval_and_payload_and_preserves_unknown_submissions() {
        let f = crate::chain_tests::fixture().await;
        enable_lending(&f, false);
        let agent = create_agent(&f.state, true, false);
        let intent = f
            .state
            .finance_prepare("owner", &agent.id, input("pool_deposit", "5"))
            .await
            .unwrap();
        assert_eq!(intent.action, "finance_pool_deposit");
        assert_eq!(intent.to, LENDING);
        assert_eq!(intent.details["finance_module"], "lending");
        assert_eq!(intent.transactions.len(), 2);
        assert_eq!(intent.transactions[0].to, bnb::USDT);
        let approval = ethabi::decode(
            &[ethabi::ParamType::Address, ethabi::ParamType::Uint(256)],
            &hex::decode(&intent.transactions[0].data[10..]).unwrap(),
        )
        .unwrap();
        assert_eq!(
            approval,
            vec![
                bnb::addr(LENDING).unwrap(),
                bnb::uint(5_000_000_000_000_000_000)
            ]
        );
        let params = abi("lending")
            .unwrap()
            .function("deposit")
            .unwrap()
            .decode_input(&hex::decode(&intent.data[10..]).unwrap())
            .unwrap();
        assert_eq!(
            params,
            vec![
                bnb::uint(5_000_000_000_000_000_000),
                bnb::addr(WALLET).unwrap()
            ]
        );
        assert_eq!(
            f.state
                .finance_prepare("owner", &agent.id, input("pool_deposit", "5"))
                .await
                .unwrap()
                .id,
            intent.id
        );
        assert!(f
            .state
            .finance_prepare("owner", &agent.id, input("pool_deposit", "6"))
            .await
            .is_err());
        assert!(f
            .state
            .finance_prepare("other", &agent.id, input("pool_deposit", "5"))
            .await
            .is_err());
        let mut expired = intent.clone();
        expired.expires_at = (Utc::now() - Duration::hours(1)).to_rfc3339();
        let submitted = bnb::hash(b"fixture-unknown-submission");
        f.state
            .store
            .connect()
            .unwrap()
            .execute(
                "UPDATE wallet_intents SET payload=?,tx_hash=? WHERE id=?",
                params![encode(&expired).unwrap(), submitted, intent.id],
            )
            .unwrap();
        let retained = f
            .state
            .finance_prepare("owner", &agent.id, input("pool_deposit", "5"))
            .await
            .unwrap();
        assert_eq!(retained.tx_hash.as_deref(), Some(submitted.as_str()));
        assert!(f
            .state
            .finance_prepare("owner", &agent.id, input("pool_redeem", "1"))
            .await
            .is_err());
        f.replies
            .lock()
            .unwrap()
            .insert(format!("code:{LENDING}"), json!("0x6001"));
        assert!(f.state.finance_verify_intent(&intent).await.is_err());
    }
    #[tokio::test]
    async fn paused_agent_and_pool_retain_lender_redemption_recovery() {
        let f = crate::chain_tests::fixture().await;
        enable_lending(&f, true);
        let agent = create_agent(&f.state, true, true);
        let intent = f
            .state
            .finance_prepare("owner", &agent.id, input("pool_redeem", "2"))
            .await
            .unwrap();
        assert_eq!(intent.transactions.len(), 1);
        let params = abi("lending")
            .unwrap()
            .function("redeem")
            .unwrap()
            .decode_input(&hex::decode(&intent.data[10..]).unwrap())
            .unwrap();
        assert_eq!(
            params,
            vec![
                bnb::uint(2_000_000_000_000_000_000),
                bnb::addr(WALLET).unwrap(),
                bnb::addr(WALLET).unwrap()
            ]
        );
        assert!(f
            .state
            .finance_prepare("owner", &agent.id, input("pool_deposit", "1"))
            .await
            .is_err());
    }
    #[tokio::test]
    async fn finance_account_returns_other_panels_pending_intents_and_owner_scoped_requests() {
        let f = crate::chain_tests::fixture().await;
        let agent = create_agent(&f.state, false, false);
        schema(&f.state.store).unwrap();
        let request = FinanceRequest {
            id: identifier(),
            loan_id: bnb::hash(b"private-request"),
            agent_id: agent.id.clone(),
            agent_name: "private".into(),
            job_id: identifier(),
            job_title: "private work".into(),
            borrower: WALLET.into(),
            amount: Decimal::ONE,
            per_call: Decimal::ONE,
            daily_cap: Decimal::ONE,
            expires_at: Utc::now().timestamp() + 3600,
            signer: WALLET.into(),
            tools: vec!["bnb-rpc".into()],
            recipients: vec![],
            status: "requested".into(),
            created_at: now(),
            public_activity: false,
            terms_hash: bnb::hash(b"private"),
        };
        f.state
            .store
            .connect()
            .unwrap()
            .execute(
                "INSERT INTO finance_requests VALUES(?,?,?,?,?,?)",
                params![
                    request.id,
                    "other",
                    agent.id,
                    request.job_id,
                    encode(&request).unwrap(),
                    request.created_at
                ],
            )
            .unwrap();
        let ix = Instruction {
            to: PROTOCOL.into(),
            data: "0x1234".into(),
            value: "0x0".into(),
        };
        let intent = TransactionIntent {
            tx_hash: None,
            id: identifier(),
            agent_id: agent.id.clone(),
            action: "back".into(),
            chain_id: 56,
            network: "mainnet".into(),
            sender: WALLET.into(),
            to: ix.to.clone(),
            data: ix.data.clone(),
            value: ix.value.clone(),
            transaction: ix.transaction(),
            transactions: vec![ix.transaction()],
            expires_at: (Utc::now() + Duration::minutes(10)).to_rfc3339(),
            details: json!({"action":"back"}),
        };
        f.state.store.connect().unwrap().execute("INSERT INTO wallet_intents(id,owner,agent_id,payload,instruction) VALUES(?,?,?,?,?)",params![intent.id,"owner",agent.id,encode(&intent).unwrap(),encode(&ix).unwrap()]).unwrap();
        let account = f.state.finance_account("owner", &agent.id).await.unwrap();
        assert_eq!(account["requests"], json!([]));
        assert_eq!(account["pending_intents"][0]["action"], "back");
        assert!(f.state.finance_account("other", &agent.id).await.is_err());
    }
    #[tokio::test]
    async fn runtime_hash_alone_cannot_verify_oracle_aggregator_or_proxy_upgrades() {
        let f = crate::chain_tests::fixture().await;
        let expected = bnb::hash(&hex::decode("60006000").unwrap());
        f.replies
            .lock()
            .unwrap()
            .insert("eth_getStorageAt".into(), json!(bnb::ZERO_HASH));
        assert!(f
            .state
            .finance_identity(LENDING, &expected, &CodePins::default(), true)
            .await
            .is_err());
        let pins = CodePins {
            aggregator: Some(WALLET.into()),
            aggregator_code_hash: Some(expected.clone()),
            ..Default::default()
        };
        f.replies.lock().unwrap().insert(
            call("aggregator()"),
            encoded(&[bnb::addr(PROTOCOL).unwrap()]),
        );
        assert!(f
            .state
            .finance_identity(LENDING, &expected, &pins, true)
            .await
            .is_err());
        f.replies
            .lock()
            .unwrap()
            .insert(call("aggregator()"), encoded(&[bnb::addr(WALLET).unwrap()]));
        f.state
            .finance_identity(LENDING, &expected, &pins, true)
            .await
            .unwrap();
        f.replies.lock().unwrap().insert(
            "eth_getStorageAt".into(),
            encoded(&[bnb::addr(PROTOCOL).unwrap()]),
        );
        assert!(f
            .state
            .finance_identity(LENDING, &expected, &pins, true)
            .await
            .is_err());
        assert!(f
            .state
            .finance_collateral_token(LENDING, Some(&expected))
            .await
            .is_err());
    }
    #[tokio::test]
    async fn same_wallet_other_agent_and_expired_submitted_jobs_block_new_finance() {
        let f = crate::chain_tests::fixture().await;
        enable_lending(&f, false);
        let first = create_agent(&f.state, true, false);
        let second = create_agent(&f.state, true, false);
        let existing = f
            .state
            .finance_prepare("owner", &first.id, input("pool_deposit", "1"))
            .await
            .unwrap();
        assert!(f
            .state
            .finance_prepare("owner", &second.id, input("pool_deposit", "1"))
            .await
            .is_err());
        let account = f.state.finance_account("owner", &second.id).await.unwrap();
        assert_eq!(account["pending_intents"][0]["agent_id"], first.id);
        f.state
            .store
            .connect()
            .unwrap()
            .execute(
                "UPDATE wallet_intents SET confirmed=2 WHERE id=?",
                [existing.id],
            )
            .unwrap();
        let ix = Instruction {
            to: PROTOCOL.into(),
            data: "0x1234".into(),
            value: "0x0".into(),
        };
        let job = JobIntent {
            tx_hash: None,
            id: identifier(),
            job_id: identifier(),
            action: "fund".into(),
            chain_id: 56,
            network: "mainnet".into(),
            sender: WALLET.into(),
            to: ix.to.clone(),
            data: ix.data.clone(),
            value: ix.value.clone(),
            approval: None,
            transactions: vec![ix.transaction()],
            expires_at: (Utc::now() - Duration::hours(1)).to_rfc3339(),
        };
        let submitted = bnb::hash(b"uncertain-job-submission");
        f.state.store.connect().unwrap().execute("INSERT INTO job_intents(id,job_id,owner,payload,tx_hash,instruction) VALUES(?,?,?,?,?,?)",params![job.id,job.job_id,"owner",encode(&job).unwrap(),submitted,encode(&ix).unwrap()]).unwrap();
        assert!(f
            .state
            .finance_prepare("owner", &second.id, input("pool_redeem", "1"))
            .await
            .is_err());
        let account = f.state.finance_account("owner", &second.id).await.unwrap();
        assert_eq!(account["pending_job_intents"][0]["tx_hash"], submitted);
        assert!(account["pending_intents"].as_array().unwrap().is_empty());
    }
    #[tokio::test]
    async fn working_capital_approval_requires_actual_chain_underwriter_before_loading_request() {
        let f = crate::chain_tests::fixture().await;
        enable_lending(&f, false);
        let mut agent = create_agent(&f.state, true, false);
        let other = "0x8888888888888888888888888888888888888888";
        agent.wallet = Some(other.into());
        agent.registry_id = Some(f.state.bnb.agent_address(other, &agent.id).unwrap());
        f.state.store.save_agent(&agent).unwrap();
        f.replies.lock().unwrap().insert(
            call("getAgent(bytes32)"),
            encoded(&[Token::Tuple(vec![
                bnb::addr(other).unwrap(),
                Token::String("willow".into()),
                bnb::uint(10_000_000_000_000_000_000),
                Token::FixedBytes(vec![1; 32]),
                Token::Bool(false),
                bnb::uint(1),
                bnb::uint(0),
                bnb::uint(0),
            ])]),
        );
        let mut input = input("advance_approve", "0");
        input.request_id = Some(identifier());
        let error = f
            .state
            .finance_prepare("owner", &agent.id, input)
            .await
            .err()
            .unwrap();
        assert_eq!(error.0.as_u16(), 403);
        assert!(error.1.contains("underwriter"));
    }
    #[tokio::test]
    async fn collateral_positions_follow_wallet_and_debt_free_withdrawal_survives_oracle_removal() {
        let f = crate::chain_tests::fixture().await;
        enable_lending(&f, false);
        let stock = "0x6666666666666666666666666666666666666666";
        let collateral = "0x9999999999999999999999999999999999999999";
        let mut config: Value =
            serde_json::from_slice(&std::fs::read(f.state.finance_config_path()).unwrap()).unwrap();
        config["modules"]["stock_loans"]["address"] = json!(stock);
        config["modules"]["stock_loans"]["code_hash"] =
            json!(bnb::hash(&hex::decode("60006000").unwrap()));
        std::fs::write(
            f.state.finance_config_path(),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let first = create_agent(&f.state, true, false);
        let second = create_agent(&f.state, true, false);
        let loan_id = bnb::hash(b"owned-collateral-recovery");
        let loan = Token::Tuple(vec![
            bnb::addr(WALLET).unwrap(),
            bnb::addr(collateral).unwrap(),
            bnb::uint(1_000_000),
            bnb::uint(0),
            bnb::uint(0),
            Token::Tuple(vec![
                bnb::addr(LENDING).unwrap(),
                bnb::addr(PROTOCOL).unwrap(),
                bnb::uint(3600),
                bnb::uint(5000),
                bnb::uint(7000),
                bnb::uint(500),
                bnb::uint(6),
                bnb::uint(8),
                bnb::uint(100_000_000_000_000_000_000),
                Token::Bool(true),
            ]),
        ]);
        f.replies
            .lock()
            .unwrap()
            .insert(call("getLoan(bytes32)"), encoded(&[loan]));
        let ix = Instruction {
            to: stock.into(),
            data: "0x1234".into(),
            value: "0x0".into(),
        };
        let prior = TransactionIntent {
            tx_hash: None,
            id: identifier(),
            agent_id: first.id.clone(),
            action: "finance_stock_borrow".into(),
            chain_id: 56,
            network: "mainnet".into(),
            sender: WALLET.into(),
            to: stock.into(),
            data: ix.data.clone(),
            value: ix.value.clone(),
            transaction: ix.transaction(),
            transactions: vec![ix.transaction()],
            expires_at: (Utc::now() - Duration::days(1)).to_rfc3339(),
            details: json!({"action":"stock_borrow","finance_module":"stock_loans","loan_id":loan_id}),
        };
        f.state.store.connect().unwrap().execute("INSERT INTO wallet_intents(id,owner,agent_id,payload,instruction,confirmed) VALUES(?,?,?,?,?,1)",params![prior.id,"owner",first.id,encode(&prior).unwrap(),encode(&ix).unwrap()]).unwrap();
        let account = f.state.finance_account("owner", &second.id).await.unwrap();
        assert_eq!(account["stock_loans"]["loans"][0]["id"], loan_id);
        assert_eq!(
            account["stock_loans"]["loans"][0]["oracle_status"],
            "unavailable"
        );
        assert_eq!(
            account["stock_loans"]["loans"][0]["actions"]["withdraw"],
            true
        );
        let mut withdraw = input("stock_withdraw", "0");
        withdraw.loan_id = Some(loan_id.clone());
        withdraw.collateral_amount = Some(Decimal::ONE);
        let recovery = f
            .state
            .finance_prepare("owner", &second.id, withdraw)
            .await
            .unwrap();
        let args = abi("stock_loans")
            .unwrap()
            .function("withdrawCollateral")
            .unwrap()
            .decode_input(&hex::decode(&recovery.data[10..]).unwrap())
            .unwrap();
        assert_eq!(
            args,
            vec![bnb::bytes32(&loan_id).unwrap(), bnb::uint(1_000_000)]
        );
        assert_eq!(recovery.transactions.len(), 1);
        assert!(f
            .replies
            .lock()
            .unwrap()
            .get(&call("loanHealth(bytes32)"))
            .is_none());
    }
}
#[derive(Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FinanceAsset {
    pub token: String,
    pub symbol: String,
    pub decimals: u8,
    pub feed: String,
    pub ltv_bps: u16,
    pub liquidation_bps: u16,
    pub max_age: u32,
    pub market_open: bool,
    pub status: String,
    pub price_usdt: Option<String>,
    pub reason: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FinanceModule {
    pub status: String,
    pub address: Option<String>,
    pub reason: Option<String>,
    pub available_usdt: Option<String>,
    pub total_assets_usdt: Option<String>,
    pub outstanding_usdt: Option<String>,
    pub total_shares: Option<String>,
    pub paused: Option<bool>,
    pub underwriter: Option<String>,
    pub assets: Vec<FinanceAsset>,
    pub official_token: Option<String>,
    pub operator: Option<String>,
    pub spent_usdt: Option<String>,
    pub tokens_burned: Option<String>,
    pub burn_method: Option<String>,
    pub token_decimals: Option<u8>,
}
#[derive(Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FinanceModules {
    pub lending: FinanceModule,
    pub stock_loans: FinanceModule,
    pub buyback: FinanceModule,
}
#[derive(Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FinanceSystem {
    pub chain_id: u64,
    pub currency: String,
    pub source: String,
    pub modules: FinanceModules,
}
#[derive(Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FinanceRequest {
    pub id: String,
    pub loan_id: String,
    pub agent_id: String,
    pub agent_name: String,
    pub job_id: String,
    pub job_title: String,
    pub borrower: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type=String)]
    pub amount: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type=String)]
    pub per_call: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type=String)]
    pub daily_cap: Decimal,
    pub expires_at: i64,
    pub signer: String,
    pub tools: Vec<String>,
    pub recipients: Vec<String>,
    pub status: String,
    pub created_at: String,
    #[serde(rename = "public")]
    pub public_activity: bool,
    pub terms_hash: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FinanceConfig {
    chain_id: u64,
    usdt_address: String,
    usdt_decimals: u8,
    modules: ModuleConfigs,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleConfigs {
    lending: ModuleConfig,
    stock_loans: ModuleConfig,
    buyback: ModuleConfig,
}
#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ModuleConfig {
    address: Option<String>,
    code_hash: Option<String>,
    assets: Vec<AssetConfig>,
    asset_history: Vec<AssetConfig>,
    usdt_feed: Option<String>,
    usdt_feed_code_hash: Option<String>,
    usdt_max_age: Option<u32>,
    usdt_feed_decimals: Option<u8>,
    usdt_feed_pins: CodePins,
    router: Option<String>,
    router_code_hash: Option<String>,
    official_token_code_hash: Option<String>,
    router_pins: CodePins,
    official_token_pins: CodePins,
    route: Vec<String>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetConfig {
    token: String,
    symbol: String,
    decimals: u8,
    token_code_hash: String,
    feed: String,
    feed_code_hash: String,
    feed_decimals: u8,
    market_status: String,
    market_status_code_hash: String,
    ltv_bps: u16,
    liquidation_bps: u16,
    bonus_bps: u16,
    max_age: u32,
    max_debt_usdt: String,
    #[serde(default)]
    feed_pins: CodePins,
    #[serde(default)]
    market_pins: CodePins,
}
#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CodePins {
    implementation: Option<String>,
    implementation_code_hash: Option<String>,
    beacon: Option<String>,
    beacon_code_hash: Option<String>,
    aggregator: Option<String>,
    aggregator_code_hash: Option<String>,
}
#[derive(Clone)]
struct VerifiedModule {
    key: String,
    address: String,
    config: ModuleConfig,
    summary: Value,
}
fn pending(reason: &str, status: &str, address: Option<String>) -> FinanceModule {
    FinanceModule {
        status: status.into(),
        address,
        reason: Some(reason.into()),
        available_usdt: None,
        total_assets_usdt: None,
        outstanding_usdt: None,
        total_shares: None,
        paused: None,
        underwriter: None,
        assets: vec![],
        official_token: None,
        operator: None,
        spent_usdt: None,
        tokens_burned: None,
        burn_method: None,
        token_decimals: None,
    }
}
fn module_for_action(action: &str) -> Result<&'static str> {
    match action {
        "pool_deposit" | "pool_redeem" | "advance_request" | "advance_approve"
        | "advance_accept" | "advance_spend" | "advance_repay" | "advance_close" => Ok("lending"),
        "stock_deposit"
        | "stock_redeem"
        | "stock_borrow"
        | "stock_add_collateral"
        | "stock_withdraw"
        | "stock_repay"
        | "stock_liquidate" => Ok("stock_loans"),
        "buyback_fund" | "buyback_execute" => Ok("buyback"),
        _ => Err(ApiError::validation("Choose a supported finance action.")),
    }
}
fn recovery(action: &str) -> bool {
    matches!(
        action,
        "pool_redeem"
            | "stock_redeem"
            | "advance_repay"
            | "advance_close"
            | "stock_add_collateral"
            | "stock_withdraw"
            | "stock_repay"
            | "stock_liquidate"
    )
}
fn abi_json(module: &str) -> Result<&'static str> {
    match module {
        "lending" => Ok(include_str!("../../contracts/bnb/abi/TabLendingPool.json")),
        "stock_loans" => Ok(include_str!("../../contracts/bnb/abi/TabStockLending.json")),
        "buyback" => Ok(include_str!("../../contracts/bnb/abi/TabBuyback.json")),
        _ => Err(ApiError::internal()),
    }
}
fn abi(module: &str) -> Result<Contract> {
    Contract::load(Cursor::new(abi_json(module)?.as_bytes())).map_err(|_| ApiError::internal())
}
fn instruction(module: &VerifiedModule, method: &str, args: Vec<Token>) -> Result<Instruction> {
    let data = abi(&module.key)?
        .function(method)
        .map_err(|_| ApiError::internal())?
        .encode_input(&args)
        .map_err(|_| {
            ApiError::validation("Finance arguments differ from the verified contract.")
        })?;
    Ok(Instruction {
        to: module.address.clone(),
        data: format!("0x{}", hex::encode(data)),
        value: "0x0".into(),
    })
}
fn value_json(token: &Token, spec: &Value) -> Value {
    match token {
        Token::Address(a) => json!(format!("{a:#x}")),
        Token::Uint(v) | Token::Int(v) => json!(v.to_string()),
        Token::Bool(v) => json!(v),
        Token::String(v) => json!(v),
        Token::Bytes(v) | Token::FixedBytes(v) => json!(format!("0x{}", hex::encode(v))),
        Token::Array(tokens) | Token::FixedArray(tokens) => json!(tokens
            .iter()
            .map(|t| value_json(t, spec))
            .collect::<Vec<_>>()),
        Token::Tuple(tokens) => {
            let mut row = serde_json::Map::new();
            for (i, t) in tokens.iter().enumerate() {
                let spec = &spec["components"][i];
                row.insert(
                    spec["name"].as_str().unwrap_or_default().into(),
                    value_json(t, spec),
                );
            }
            Value::Object(row)
        }
    }
}
fn required<'a>(value: &'a Option<String>, message: &str) -> Result<&'a str> {
    value
        .as_deref()
        .ok_or_else(|| ApiError::validation(message))
}
fn positive(amount: Decimal, decimals: u8) -> Result<u128> {
    let value = token_units(amount, decimals)?;
    if value == 0 {
        return Err(ApiError::validation(
            "Choose a positive exact token amount.",
        ));
    }
    Ok(value)
}
fn decimal_units(value: u128, decimals: u8) -> String {
    Decimal::from_i128_with_scale(value as i128, decimals.into())
        .normalize()
        .to_string()
}
fn nonzero_hash(value: &str) -> Result<Token> {
    if bnb::id(value)? == bnb::ZERO_HASH {
        return Err(ApiError::validation("Use a nonzero commitment hash."));
    }
    bnb::bytes32(value)
}
fn schema(store: &Store) -> Result<()> {
    store.connect()?.execute_batch("CREATE TABLE IF NOT EXISTS finance_requests(id TEXT PRIMARY KEY,owner TEXT NOT NULL,agent_id TEXT NOT NULL,job_id TEXT NOT NULL,payload TEXT NOT NULL,created_at TEXT NOT NULL); CREATE INDEX IF NOT EXISTS finance_requests_owner ON finance_requests(owner,agent_id);")?;
    Ok(())
}
impl AppState {
    fn finance_pending_jobs(&self, owner: &str, wallet: &str) -> Result<Vec<JobActionRecord>> {
        let db = self.store.connect()?;
        let mut statement=db.prepare("SELECT payload,tx_hash,confirmed FROM job_intents WHERE owner=? AND lower(json_extract(payload,'$.sender'))=lower(?) AND confirmed=0 AND (tx_hash IS NOT NULL OR julianday(json_extract(payload,'$.expires_at'))>julianday(?)) ORDER BY rowid DESC")?;
        let rows = statement.query_map(params![owner, wallet, now()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, u8>(2)?,
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
    fn finance_config_path(&self) -> PathBuf {
        std::env::var("TAB_FINANCE_CONFIG")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                self.config
                    .merchants
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join("finance-bnb.json")
            })
    }
    fn finance_config(&self) -> Result<FinanceConfig> {
        let path = self.finance_config_path();
        let bytes = if path.exists() {
            std::fs::read(path)
                .map_err(|_| ApiError::unavailable("Finance configuration is unavailable."))?
        } else {
            include_bytes!("../config/finance-bnb.json").to_vec()
        };
        if bytes.len() > 64 * 1024 {
            return Err(ApiError::unavailable(
                "Finance configuration exceeds supported limits.",
            ));
        }
        let config: FinanceConfig = serde_json::from_slice(&bytes)
            .map_err(|_| ApiError::unavailable("Finance configuration is invalid."))?;
        if config.chain_id != 56
            || config.usdt_decimals != 18
            || bnb::address(&config.usdt_address)? != bnb::USDT
        {
            return Err(ApiError::unavailable(
                "Finance configuration must use BNB mainnet and canonical 18-decimal USDT.",
            ));
        }
        Ok(config)
    }
    async fn finance_view(
        &self,
        module: &VerifiedModule,
        method: &str,
        args: Vec<Token>,
    ) -> Result<Value> {
        let ix = instruction(module, method, args)?;
        let data = self
            .bnb
            .rpc("eth_call", json!([{"to":ix.to,"data":ix.data},"latest"]))
            .await?;
        let bytes = hex::decode(
            data.as_str()
                .and_then(|v| v.strip_prefix("0x"))
                .ok_or_else(|| ApiError::unavailable("Invalid finance response."))?,
        )
        .map_err(|_| ApiError::unavailable("Invalid finance response."))?;
        let values = abi(&module.key)?
            .function(method)
            .map_err(|_| ApiError::internal())?
            .decode_output(&bytes)
            .map_err(|_| {
                ApiError::unavailable("Finance response differs from the verified ABI.")
            })?;
        let spec: Value = serde_json::from_str(abi_json(&module.key)?)?;
        let function = spec
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find(|f| f["type"] == "function" && f["name"] == method)
            })
            .ok_or_else(ApiError::internal)?;
        if values.len() == 1 {
            return Ok(value_json(&values[0], &function["outputs"][0]));
        }
        Ok(json!(values
            .iter()
            .enumerate()
            .map(|(i, t)| value_json(t, &function["outputs"][i]))
            .collect::<Vec<_>>()))
    }
    async fn finance_code(&self, address: &str, expected: &str) -> Result<()> {
        bnb::signature(expected)?;
        let code = self
            .bnb
            .rpc("eth_getCode", json!([bnb::address(address)?, "latest"]))
            .await?;
        let bytes = hex::decode(
            code.as_str()
                .and_then(|v| v.strip_prefix("0x"))
                .ok_or_else(|| ApiError::unavailable("Invalid finance bytecode."))?,
        )
        .map_err(|_| ApiError::unavailable("Invalid finance bytecode."))?;
        if bytes.is_empty() || bnb::hash(&bytes) != expected.to_lowercase() {
            return Err(ApiError::unavailable(
                "Finance code hash changed or the contract is not deployed.",
            ));
        }
        Ok(())
    }
    async fn finance_address_call(&self, address: &str, signature: &str) -> Result<String> {
        let selector = &bnb::hash(signature.as_bytes())[2..10];
        let raw = self
            .bnb
            .rpc(
                "eth_call",
                json!([{"to":bnb::address(address)?,"data":format!("0x{selector}")},"latest"]),
            )
            .await?;
        let word = raw
            .as_str()
            .filter(|v| v.len() == 66 && v.starts_with("0x") && v[2..26].chars().all(|c| c == '0'))
            .ok_or_else(|| ApiError::unavailable("Invalid oracle or proxy identity response."))?;
        bnb::address(&format!("0x{}", &word[26..]))
    }
    async fn finance_slot_address(&self, address: &str, slot: &str) -> Result<String> {
        let raw = self
            .bnb
            .rpc(
                "eth_getStorageAt",
                json!([bnb::address(address)?, slot, "latest"]),
            )
            .await?;
        let word = raw
            .as_str()
            .filter(|v| v.len() == 66 && v.starts_with("0x") && v[2..26].chars().all(|c| c == '0'))
            .ok_or_else(|| ApiError::unavailable("Invalid proxy storage response."))?;
        bnb::address(&format!("0x{}", &word[26..]))
    }
    async fn finance_identity(
        &self,
        address: &str,
        expected: &str,
        pins: &CodePins,
        oracle: bool,
    ) -> Result<()> {
        self.finance_code(address, expected).await?;
        let implementation = self
            .finance_slot_address(
                address,
                "0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc",
            )
            .await?;
        let beacon = self
            .finance_slot_address(
                address,
                "0xa3f0ad74e5423aebfd80d3ef4346578335a9a72aeaee59ff6cb3582b35133d50",
            )
            .await?;
        let implementation = if beacon != bnb::ZERO {
            if bnb::address(required(
                &pins.beacon,
                "Pin the proxy beacon before using this asset.",
            )?)? != beacon
            {
                return Err(ApiError::unavailable("Proxy beacon changed."));
            }
            self.finance_code(
                &beacon,
                required(&pins.beacon_code_hash, "Pin the beacon runtime hash.")?,
            )
            .await?;
            self.finance_address_call(&beacon, "implementation()")
                .await?
        } else {
            implementation
        };
        if implementation != bnb::ZERO {
            if bnb::address(required(
                &pins.implementation,
                "Pin the proxy implementation before using this asset.",
            )?)? != implementation
            {
                return Err(ApiError::unavailable("Proxy implementation changed."));
            }
            self.finance_code(
                &implementation,
                required(
                    &pins.implementation_code_hash,
                    "Pin the implementation runtime hash.",
                )?,
            )
            .await?;
        } else if pins.implementation.is_some() || pins.beacon.is_some() {
            return Err(ApiError::unavailable("Configured proxy identity changed."));
        }
        if oracle {
            // Supported feed proxies require an exact aggregator pin. Runtime
            // bytecode alone cannot detect a Chainlink aggregator replacement.
            let aggregator = bnb::address(required(
                &pins.aggregator,
                "Pin the price-feed aggregator before enabling collateral.",
            )?)?;
            if self.finance_address_call(address, "aggregator()").await? != aggregator {
                return Err(ApiError::unavailable("Oracle aggregator changed."));
            }
            self.finance_code(
                &aggregator,
                required(
                    &pins.aggregator_code_hash,
                    "Pin the price-feed aggregator runtime hash.",
                )?,
            )
            .await?;
            for slot in [
                "0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc",
                "0xa3f0ad74e5423aebfd80d3ef4346578335a9a72aeaee59ff6cb3582b35133d50",
            ] {
                if self.finance_slot_address(&aggregator, slot).await? != bnb::ZERO {
                    return Err(ApiError::unavailable("An upgradeable oracle aggregator needs an additional reviewed implementation policy."));
                }
            }
        }
        Ok(())
    }
    async fn finance_collateral_token(
        &self,
        token: &str,
        expected_hash: Option<&str>,
    ) -> Result<()> {
        let catalog = self.backing_assets();
        let asset = catalog
            .as_array()
            .and_then(|assets| {
                assets.iter().find(|a| {
                    a["enabled"] == true
                        && a["address"]
                            .as_str()
                            .is_some_and(|a| a.eq_ignore_ascii_case(token))
                })
            })
            .ok_or_else(|| {
                ApiError::unavailable(
                    "Collateral custody identity is absent from the reviewed token catalog.",
                )
            })?;
        if expected_hash.is_some_and(|h| {
            asset["code_hash"]
                .as_str()
                .is_none_or(|actual| !h.eq_ignore_ascii_case(actual))
        }) {
            return Err(ApiError::unavailable(
                "Finance and custody token identities differ.",
            ));
        }
        self.bnb.verify_backing_asset(asset).await
    }
    async fn finance_stock_oracle(&self, module: &VerifiedModule) -> Result<()> {
        let feed = required(
            &module.config.usdt_feed,
            "Configure the exact USDT price feed.",
        )?;
        self.finance_identity(
            feed,
            required(
                &module.config.usdt_feed_code_hash,
                "Configure the USDT feed hash.",
            )?,
            &module.config.usdt_feed_pins,
            true,
        )
        .await?;
        let decimals = module
            .config
            .usdt_feed_decimals
            .ok_or_else(|| ApiError::unavailable("Pin the USDT feed precision."))?;
        if self.finance_view(module, "usdtUsdFeed", vec![]).await? != bnb::address(feed)?
            || bnb::number(&self.finance_view(module, "usdtMaxAge", vec![]).await?)?
                != u128::from(module.config.usdt_max_age.unwrap_or(0))
            || decimals > 18
            || self.bnb.token_decimals(feed).await? != decimals
            || bnb::number(
                &self
                    .finance_view(module, "usdtFeedDecimals", vec![])
                    .await?,
            )? != u128::from(decimals)
        {
            return Err(ApiError::unavailable(
                "The USDT oracle differs from the approved configuration.",
            ));
        }
        Ok(())
    }
    async fn finance_historical_asset(&self, module: &VerifiedModule, loan: &Value) -> Result<()> {
        let terms = &loan["terms"];
        let asset = module
            .config
            .assets
            .iter()
            .chain(&module.config.asset_history)
            .find(|a| {
                loan["collateralToken"]
                    .as_str()
                    .is_some_and(|t| t.eq_ignore_ascii_case(&a.token))
                    && terms["feed"] == a.feed
                    && terms["marketStatus"] == a.market_status
                    && bnb::number(&terms["tokenDecimals"]).ok() == Some(a.decimals.into())
                    && bnb::number(&terms["feedDecimals"]).ok() == Some(a.feed_decimals.into())
                    && bnb::number(&terms["borrowLtvBps"]).ok() == Some(a.ltv_bps.into())
                    && bnb::number(&terms["liquidationLtvBps"]).ok()
                        == Some(a.liquidation_bps.into())
                    && bnb::number(&terms["liquidationBonusBps"]).ok() == Some(a.bonus_bps.into())
                    && bnb::number(&terms["maxAge"]).ok() == Some(a.max_age.into())
            })
            .ok_or_else(|| {
                ApiError::unavailable(
                    "This loan's snapshotted oracle policy is not in the approved history.",
                )
            })?;
        self.finance_stock_oracle(module).await?;
        self.finance_collateral_token(&asset.token, Some(&asset.token_code_hash))
            .await?;
        self.finance_identity(&asset.feed, &asset.feed_code_hash, &asset.feed_pins, true)
            .await?;
        if self.bnb.token_decimals(&asset.feed).await? != asset.feed_decimals {
            return Err(ApiError::unavailable(
                "Snapshotted collateral oracle precision changed.",
            ));
        }
        self.finance_identity(
            &asset.market_status,
            &asset.market_status_code_hash,
            &asset.market_pins,
            false,
        )
        .await
    }
    async fn finance_module(&self, key: &str) -> Result<VerifiedModule> {
        let config = self.finance_config()?;
        let cfg = match key {
            "lending" => config.modules.lending,
            "stock_loans" => config.modules.stock_loans,
            "buyback" => config.modules.buyback,
            _ => return Err(ApiError::validation("Unknown finance module.")),
        };
        let address = bnb::address(cfg.address.as_deref().ok_or_else(|| {
            ApiError::unavailable("This finance module is awaiting deployment and verification.")
        })?)?;
        if address == bnb::ZERO {
            return Err(ApiError::unavailable(
                "Finance module address is not configured.",
            ));
        }
        let expected = cfg
            .code_hash
            .as_deref()
            .ok_or_else(|| ApiError::unavailable("Finance runtime hash is not configured."))?;
        self.bnb.require_deployment().await?;
        self.finance_code(&address, expected).await?;
        let mut module = VerifiedModule {
            key: key.into(),
            address,
            config: cfg,
            summary: Value::Null,
        };
        if self.finance_view(&module, "protocol", vec![]).await? != self.config.program
            || self.finance_view(&module, "usdt", vec![]).await? != bnb::USDT
        {
            return Err(ApiError::unavailable(
                "Finance module belongs to a different protocol or settlement token.",
            ));
        }
        let authority = self.bnb.view("protocol", "authority", vec![]).await?;
        module.summary = self
            .finance_view(
                &module,
                if key == "buyback" {
                    "getBuyback"
                } else {
                    "getPool"
                },
                vec![],
            )
            .await?;
        if module.summary["asset"] != bnb::USDT
            || module.summary[if key == "buyback" {
                "operator"
            } else {
                "underwriter"
            }] != authority
        {
            return Err(ApiError::unavailable(
                "Finance authority or asset differs from the verified protocol.",
            ));
        }
        if key == "buyback" {
            let token = self.config.official_tab.as_deref().ok_or_else(|| {
                ApiError::unavailable("Configure and verify official TAB before enabling buybacks.")
            })?;
            self.finance_identity(
                token,
                required(
                    &module.config.official_token_code_hash,
                    "Configure the official TAB runtime hash.",
                )?,
                &module.config.official_token_pins,
                false,
            )
            .await?;
            let router = required(&module.config.router, "Configure the exact buyback router.")?;
            self.finance_identity(
                router,
                required(
                    &module.config.router_code_hash,
                    "Configure the buyback router hash.",
                )?,
                &module.config.router_pins,
                false,
            )
            .await?;
            let route = module
                .config
                .route
                .iter()
                .map(|a| bnb::address(a))
                .collect::<Result<Vec<_>>>()?;
            if module.summary["token"] != token
                || module.summary["router"] != bnb::address(router)?
                || module.summary["route"] != json!(route)
                || route.first().map(String::as_str) != Some(bnb::USDT)
                || route.last().map(String::as_str) != Some(token)
            {
                return Err(ApiError::unavailable(
                    "Buyback route differs from the verified official-token route.",
                ));
            }
        }
        Ok(module)
    }
    async fn finance_asset(
        &self,
        module: &VerifiedModule,
        token: &str,
    ) -> Result<(AssetConfig, Value)> {
        let token = bnb::address(token)?;
        let asset = module
            .config
            .assets
            .iter()
            .find(|a| a.token.eq_ignore_ascii_case(&token))
            .cloned()
            .ok_or_else(|| {
                ApiError::validation(
                    "Choose a verified collateral token from the operator whitelist.",
                )
            })?;
        if asset.decimals > 18
            || asset.feed_decimals > 18
            || asset.ltv_bps == 0
            || asset.ltv_bps >= asset.liquidation_bps
            || asset.liquidation_bps > 9000
            || !(60..=86400).contains(&asset.max_age)
            || asset.bonus_bps > 1500
        {
            return Err(ApiError::unavailable(
                "Collateral risk configuration is invalid.",
            ));
        }
        self.finance_stock_oracle(module).await?;
        self.finance_collateral_token(&asset.token, Some(&asset.token_code_hash))
            .await?;
        self.finance_identity(&asset.feed, &asset.feed_code_hash, &asset.feed_pins, true)
            .await?;
        if self.bnb.token_decimals(&asset.feed).await? != asset.feed_decimals {
            return Err(ApiError::unavailable(
                "Collateral oracle precision changed.",
            ));
        }
        self.finance_identity(
            &asset.market_status,
            &asset.market_status_code_hash,
            &asset.market_pins,
            false,
        )
        .await?;
        let c = self
            .finance_view(module, "getAsset", vec![bnb::addr(&token)?])
            .await?;
        if c["enabled"] != true
            || c["feed"] != bnb::address(&asset.feed)?
            || c["marketStatus"] != bnb::address(&asset.market_status)?
            || bnb::number(&c["tokenDecimals"])? != u128::from(asset.decimals)
            || bnb::number(&c["feedDecimals"])? != u128::from(asset.feed_decimals)
            || self.bnb.token_decimals(&token).await? != asset.decimals
            || bnb::number(&c["borrowLtvBps"])? != u128::from(asset.ltv_bps)
            || bnb::number(&c["liquidationLtvBps"])? != u128::from(asset.liquidation_bps)
            || bnb::number(&c["liquidationBonusBps"])? != u128::from(asset.bonus_bps)
            || bnb::number(&c["maxAge"])? != u128::from(asset.max_age)
            || bnb::number(&c["maxDebt"])?
                != units(
                    asset
                        .max_debt_usdt
                        .parse()
                        .map_err(|_| ApiError::unavailable("Invalid collateral debt ceiling."))?,
                )?
        {
            return Err(ApiError::unavailable(
                "Onchain collateral rules differ from the approved configuration.",
            ));
        }
        Ok((asset, c))
    }
    async fn finance_module_summary(&self, key: &str, cfg: &ModuleConfig) -> FinanceModule {
        if cfg.address.is_none() {
            return pending("Deployment and funding are pending.", "not_deployed", None);
        }
        let result = tokio::time::timeout(std::time::Duration::from_secs(7), async {
            let module = self.finance_module(key).await?;
            let p = &module.summary;
            let mut row = pending("", "verified", Some(module.address.clone()));
            row.reason = None;
            row.paused = p["paused"].as_bool();
            if key == "buyback" {
                row.official_token = p["token"].as_str().map(str::to_owned);
                row.operator = p["operator"].as_str().map(str::to_owned);
                row.available_usdt = Some(
                    money(bnb::number(
                        &self.finance_view(&module, "availableUsdt", vec![]).await?,
                    )?)
                    .to_string(),
                );
                row.spent_usdt = Some(money(bnb::number(&p["spentUsdt"])?).to_string());
                let decimals = self
                    .bnb
                    .token_decimals(row.official_token.as_deref().expect("verified token"))
                    .await?;
                row.token_decimals = Some(decimals);
                row.tokens_burned =
                    Some(decimal_units(bnb::number(&p["tokensAcquired"])?, decimals));
                row.burn_method = Some("dead_address_transfer".into());
            } else {
                row.underwriter = p["underwriter"].as_str().map(str::to_owned);
                let available = bnb::number(&p["liquidity"])?
                    .checked_sub(bnb::number(&p["reserved"])?)
                    .ok_or_else(|| ApiError::unavailable("Invalid pool accounting."))?;
                row.available_usdt = Some(money(available).to_string());
                row.total_assets_usdt = Some(money(bnb::number(&p["assets"])?).to_string());
                row.outstanding_usdt = Some(money(bnb::number(&p["outstanding"])?).to_string());
                row.total_shares = Some(money(bnb::number(&p["shares"])?).to_string());
                if key == "stock_loans" {
                    if module.config.assets.len() > 16 {
                        return Err(ApiError::unavailable(
                            "Collateral whitelist exceeds supported limits.",
                        ));
                    }
                    for asset in &module.config.assets {
                        let checked = async {
                            let (asset, _) = self.finance_asset(&module, &asset.token).await?;
                            let open = self
                                .finance_view(&module, "marketOpen", vec![bnb::addr(&asset.token)?])
                                .await?
                                == true;
                            let quote = self
                                .finance_view(
                                    &module,
                                    "collateralQuote",
                                    vec![
                                        bnb::addr(&asset.token)?,
                                        bnb::uint(10u128.pow(asset.decimals.into())),
                                    ],
                                )
                                .await?;
                            Ok::<_, ApiError>((open, money(bnb::number(&quote[0])?).to_string()))
                        };
                        let checked =
                            tokio::time::timeout(std::time::Duration::from_secs(2), checked)
                                .await
                                .ok()
                                .and_then(|v| v.ok());
                        row.assets.push(FinanceAsset {
                            token: asset.token.clone(),
                            symbol: asset.symbol.clone(),
                            decimals: asset.decimals,
                            feed: asset.feed.clone(),
                            ltv_bps: asset.ltv_bps,
                            liquidation_bps: asset.liquidation_bps,
                            max_age: asset.max_age,
                            market_open: checked.as_ref().is_some_and(|v| v.0),
                            status: if checked.is_some() {
                                "verified"
                            } else {
                                "unavailable"
                            }
                            .into(),
                            price_usdt: checked.map(|v| v.1),
                            reason: None,
                        });
                    }
                }
            }
            Ok::<_, ApiError>(row)
        })
        .await;
        result.ok().and_then(|v| v.ok()).unwrap_or_else(|| {
            pending(
                "Onchain configuration could not be verified. Wallet actions are disabled.",
                "unavailable",
                cfg.address.clone(),
            )
        })
    }
    pub async fn finance_system(&self) -> FinanceSystem {
        let modules = match self.finance_config() {
            Ok(c) => {
                let (lending, stock_loans, buyback) = tokio::join!(
                    self.finance_module_summary("lending", &c.modules.lending),
                    self.finance_module_summary("stock_loans", &c.modules.stock_loans),
                    self.finance_module_summary("buyback", &c.modules.buyback)
                );
                FinanceModules {
                    lending,
                    stock_loans,
                    buyback,
                }
            }
            Err(_) => FinanceModules {
                lending: pending("Finance configuration is unavailable.", "unavailable", None),
                stock_loans: pending("Finance configuration is unavailable.", "unavailable", None),
                buyback: pending("Finance configuration is unavailable.", "unavailable", None),
            },
        };
        let live = [&modules.lending, &modules.stock_loans, &modules.buyback]
            .iter()
            .any(|m| m.status == "verified");
        FinanceSystem {
            chain_id: 56,
            currency: "USDT".into(),
            source: if live {
                "verified_onchain"
            } else {
                "deployment_pending"
            }
            .into(),
            modules,
        }
    }
    pub async fn finance_quote(&self, input: FinanceInput) -> Result<Value> {
        let module = self
            .finance_module(module_for_action(&input.action)?)
            .await?;
        match input.action.as_str() {
            "stock_borrow" => {
                let token = required(&input.token_address, "Choose collateral.")?;
                let (asset, _) = self.finance_asset(&module, token).await?;
                let amount = positive(
                    input
                        .collateral_amount
                        .ok_or_else(|| ApiError::validation("Set the collateral amount."))?,
                    asset.decimals,
                )?;
                let q = self
                    .finance_view(
                        &module,
                        "collateralQuote",
                        vec![bnb::addr(token)?, bnb::uint(amount)],
                    )
                    .await?;
                let price = self
                    .finance_view(
                        &module,
                        "collateralQuote",
                        vec![
                            bnb::addr(token)?,
                            bnb::uint(10u128.pow(asset.decimals.into())),
                        ],
                    )
                    .await?;
                Ok(
                    json!({"status":"verified","action":input.action,"chain_id":56,"currency":"USDT","token":bnb::address(token)?,"decimals":asset.decimals,"collateral_value_usdt":money(bnb::number(&q[0])?).to_string(),"maximum_borrow_usdt":money(bnb::number(&q[1])?).to_string(),"liquidation_debt_usdt":money(bnb::number(&q[2])?).to_string(),"price_usdt":money(bnb::number(&price[0])?).to_string()}),
                )
            }
            "stock_liquidate" => {
                let id = required(&input.loan_id, "Choose the exact loan.")?;
                let loan = self
                    .finance_view(&module, "getLoan", vec![bnb::bytes32(id)?])
                    .await?;
                self.finance_historical_asset(&module, &loan).await?;
                let decimals = bnb::number(&loan["terms"]["tokenDecimals"])? as u8;
                if decimals > 18 || loan["borrower"] == bnb::ZERO {
                    return Err(ApiError::missing("Collateral loan not found."));
                }
                let q = self
                    .finance_view(
                        &module,
                        "liquidationQuote",
                        vec![bnb::bytes32(id)?, bnb::uint(positive(input.amount, 18)?)],
                    )
                    .await?;
                Ok(
                    json!({"status":"verified","action":input.action,"chain_id":56,"currency":"USDT","maximum_repay_usdt":money(bnb::number(&q[0])?).to_string(),"collateral_out":decimal_units(bnb::number(&q[1])?,decimals),"token":loan["collateralToken"],"decimals":decimals}),
                )
            }
            "buyback_execute" => {
                let q = self
                    .finance_view(
                        &module,
                        "quote",
                        vec![bnb::uint(positive(input.amount, 18)?)],
                    )
                    .await?;
                let token = module.summary["token"]
                    .as_str()
                    .ok_or_else(ApiError::internal)?;
                let decimals = self.bnb.token_decimals(token).await?;
                Ok(
                    json!({"status":"verified","action":input.action,"chain_id":56,"currency":"USDT","quoted_tokens":decimal_units(bnb::number(&q[0])?,decimals),"minimum_tokens":decimal_units(bnb::number(&q[1])?,decimals),"token":token,"decimals":decimals,"deadline":Utc::now().timestamp()+600}),
                )
            }
            _ => Err(ApiError::validation(
                "This action does not provide a loan quote.",
            )),
        }
    }
    async fn finance_request_terms(
        &self,
        agent: &RuntimeAgent,
        input: &FinanceInput,
        module: &VerifiedModule,
    ) -> Result<(String, Job, Value)> {
        if agent.status != "ready" {
            return Err(ApiError::conflict(
                "Register or resume the agent before requesting working capital.",
            ));
        }
        let wallet = self.verify_owned_agent(agent).await?;
        let key = agent.registry_id.as_deref().expect("verified agent");
        let job_id = required(
            &input.job_id,
            "Choose the funded job that this advance will support.",
        )?;
        let job = Store::job(&self.store.connect()?, job_id)?;
        if job.plan.executor_id != agent.id || job.executor_wallet != wallet {
            return Err(ApiError::forbidden(
                "Working capital belongs to the job's exact registered executor.",
            ));
        }
        let chain = self.bnb.job(job_id).await?;
        let agent_chain = self.bnb.agent(key).await?;
        let root = self
            .bnb
            .view(
                "protocol",
                "getJob",
                vec![bnb::bytes32(
                    chain["root"].as_str().ok_or_else(ApiError::internal)?,
                )?],
            )
            .await?;
        let now = Utc::now().timestamp();
        let expires = input
            .expires_at
            .ok_or_else(|| ApiError::validation("Set the advance expiry."))?;
        let per = input
            .per_call
            .ok_or_else(|| ApiError::validation("Set the per-call advance limit."))?;
        let daily = input
            .daily_cap
            .ok_or_else(|| ApiError::validation("Set the daily advance limit."))?;
        let principal = positive(input.amount, 18)?;
        let per_units = positive(per, 18)?;
        let daily_units = positive(daily, 18)?;
        if module.summary["paused"] == true
            || agent_chain["paused"] == true
            || chain["paused"] == true
            || root["paused"] == true
            || bnb::number(&chain["state"])? != 1
            || chain["agent"] != key
            || chain["executor"] != wallet
            || chain["termsHash"] != job.terms_hash
            || principal > bnb::number(&chain["available"])?
            || input.amount > Decimal::from(10000)
            || per > agent.plan.max_call
            || per_units > bnb::number(&chain["maxCall"])?
            || per > daily
            || daily > input.amount
            || daily > agent.plan.daily_cap
            || daily_units > bnb::number(&agent_chain["dailyCap"])?
            || expires <= now
            || expires > now + 31 * 86400
            || expires as u128 > bnb::number(&chain["deadline"])?
        {
            return Err(ApiError::validation(
                "Keep working capital within the funded job, live agent policy, and job deadline.",
            ));
        }
        let signer = bnb::address(required(
            &input.signer,
            "Choose the authorized spend signer.",
        )?)?;
        if signer == bnb::ZERO
            || input.tools.is_empty()
            || !distinct(&input.tools)
            || input
                .tools
                .iter()
                .any(|t| !agent.plan.tools.contains(t) || !job.plan.tools.contains(t))
            || (u128::from(tool_bitmap(&input.tools)) & !bnb::number(&chain["tools"])?) != 0
        {
            return Err(ApiError::validation(
                "Choose a signer and a subset of the approved job tools.",
            ));
        }
        let merchants = self.merchants()?;
        let allowed: BTreeSet<_> = merchants
            .iter()
            .filter(|m| input.tools.contains(&m.tool))
            .map(|m| m.recipient.to_lowercase())
            .collect();
        let recipients = input
            .recipients
            .iter()
            .map(|r| bnb::address(r))
            .collect::<Result<Vec<_>>>()?;
        let underwriter = module.summary["underwriter"]
            .as_str()
            .ok_or_else(ApiError::internal)?;
        let job_recipients = chain["recipients"]
            .as_array()
            .ok_or_else(|| ApiError::unavailable("Job recipient policy is unavailable."))?;
        if recipients.is_empty()
            || recipients.len() > 16
            || !distinct(&recipients)
            || recipients.iter().any(|r| {
                !allowed.contains(r)
                    || !job_recipients.iter().any(|v| v == r)
                    || [
                        bnb::ZERO,
                        module.address.as_str(),
                        self.config.program.as_str(),
                        wallet.as_str(),
                        signer.as_str(),
                        underwriter,
                    ]
                    .contains(&r.as_str())
            })
        {
            return Err(ApiError::validation("Choose distinct operator-approved service recipients from this job's exact allowlist."));
        }
        Ok((wallet, job, chain))
    }
    pub async fn finance_request(
        &self,
        owner: &str,
        id: &str,
        mut input: FinanceInput,
    ) -> Result<FinanceRequest> {
        if input.action != "advance_request" {
            return Err(ApiError::validation(
                "Choose advance_request for a working-capital request.",
            ));
        }
        let agent = self.store.agent(owner, id)?;
        let module = self.finance_module("lending").await?;
        let (wallet, job, _) = self.finance_request_terms(&agent, &input, &module).await?;
        input.signer = Some(bnb::address(
            input.signer.as_deref().expect("validated signer"),
        )?);
        input.recipients = input
            .recipients
            .iter()
            .map(|r| bnb::address(r))
            .collect::<Result<Vec<_>>>()?;
        if self
            .finance_view(&module, "jobLoan", vec![bnb::bytes32(&job.id)?])
            .await?
            != bnb::ZERO_HASH
        {
            return Err(ApiError::conflict(
                "This job already has a working-capital line.",
            ));
        }
        schema(&self.store)?;
        let mut db = self.store.connect()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT payload FROM finance_requests WHERE agent_id=? AND job_id=?",
                params![id, job.id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(payload) = existing {
            let prior: FinanceRequest = decode(payload)?;
            if prior.amount == input.amount
                && prior.per_call == input.per_call.expect("validated")
                && prior.daily_cap == input.daily_cap.expect("validated")
                && Some(prior.expires_at) == input.expires_at
                && Some(&prior.signer) == input.signer.as_ref()
                && prior.tools == input.tools
                && prior.recipients == input.recipients
            {
                return Ok(prior);
            }
            return Err(ApiError::conflict("This job already has an immutable working-capital request. Review its original terms."));
        }
        let request_id = identifier();
        let request = FinanceRequest {
            id: request_id.clone(),
            loan_id: bnb::hash(format!("tab-working-capital:{request_id}").as_bytes()),
            agent_id: id.into(),
            agent_name: agent.plan.name,
            job_id: job.id,
            job_title: job.plan.title,
            borrower: wallet,
            amount: input.amount,
            per_call: input.per_call.expect("validated"),
            daily_cap: input.daily_cap.expect("validated"),
            expires_at: input.expires_at.expect("validated"),
            signer: input.signer.expect("validated"),
            tools: input.tools,
            recipients: input.recipients,
            status: "requested".into(),
            created_at: now(),
            public_activity: input.public_activity,
            terms_hash: job.terms_hash,
        };
        tx.execute("INSERT INTO finance_requests(id,owner,agent_id,job_id,payload,created_at) VALUES(?,?,?,?,?,?)",params![request.id,owner,id,request.job_id,encode(&request)?,request.created_at])?;
        tx.commit()?;
        Ok(request)
    }
    pub async fn finance_requests(&self, owner: &str, id: &str) -> Result<Value> {
        let agent = self.store.agent(owner, id)?;
        schema(&self.store)?;
        let underwriter = if agent.registry_id.is_some() {
            if let Ok(module) = self.finance_module("lending").await {
                self.verify_owned_agent(&agent)
                    .await
                    .ok()
                    .is_some_and(|w| module.summary["underwriter"] == w)
            } else {
                false
            }
        } else {
            false
        };
        let mut requests: Vec<FinanceRequest> = if underwriter {
            Store::list_payload(
                &self.store.connect()?,
                "SELECT payload FROM finance_requests ORDER BY created_at DESC LIMIT 100",
                [],
            )?
        } else {
            Store::list_payload(&self.store.connect()?,"SELECT payload FROM finance_requests WHERE owner=? AND agent_id=? ORDER BY created_at DESC LIMIT 100",params![owner,id])?
        };
        if let Ok(module) = self.finance_module("lending").await {
            for request in &mut requests {
                if let Ok(loan) = self
                    .finance_view(&module, "getLoan", vec![bnb::bytes32(&request.loan_id)?])
                    .await
                {
                    if loan["borrower"] == request.borrower {
                        request.status = if loan["closed"] == true {
                            "closed"
                        } else if loan["accepted"] == true {
                            "accepted"
                        } else {
                            "approved"
                        }
                        .into();
                    }
                }
            }
        }
        Ok(json!({"requests":requests,"underwriter":underwriter}))
    }
    pub async fn finance_prepare(
        &self,
        owner: &str,
        id: &str,
        mut input: FinanceInput,
    ) -> Result<TransactionIntent> {
        let key = module_for_action(&input.action)?;
        if input.action == "advance_request" {
            return Err(ApiError::validation(
                "Submit requests through the working-capital request route.",
            ));
        }
        let agent = self.store.agent(owner, id)?;
        let module = self.finance_module(key).await?;
        if agent.status != "ready" && !(agent.status == "paused" && recovery(&input.action)) {
            return Err(ApiError::conflict(
                "Register or resume the agent before this finance action.",
            ));
        }
        let wallet = self.verify_owned_agent(&agent).await?;
        let agent_key = agent.registry_id.as_deref().expect("verified agent");
        let chain_agent = self.bnb.agent(agent_key).await?;
        if chain_agent["paused"] == true && !recovery(&input.action) {
            return Err(ApiError::conflict(
                "Resume the onchain agent before a new financial commitment.",
            ));
        }
        let input_hash = digest(&serde_json::to_value(&input)?);
        let action = format!("finance_{}", input.action);
        if !self.finance_pending_jobs(owner, &wallet)?.is_empty() {
            return Err(ApiError::conflict(
                "Confirm the existing job wallet action before preparing another finance action.",
            ));
        }
        let pending:Vec<TransactionIntent>=Store::list_payload(&self.store.connect()?,"SELECT json_set(payload,'$.tx_hash',tx_hash) FROM wallet_intents WHERE owner=? AND lower(json_extract(payload,'$.sender'))=lower(?) AND confirmed=0 AND (tx_hash IS NOT NULL OR julianday(json_extract(payload,'$.expires_at'))>julianday(?))",params![owner,wallet,now()])?;
        for intent in pending {
            if intent.agent_id == id
                && intent.action == action
                && intent.details["input_hash"] == input_hash
            {
                return Ok(intent);
            }
            return Err(ApiError::conflict(
                "Confirm the existing wallet action before preparing another.",
            ));
        }
        if crate::intents::has_pending_transaction(&self.store.connect()?, owner, &wallet)? {
            return Err(ApiError::conflict(
                "Confirm the existing transaction for this wallet before preparing another.",
            ));
        }
        let mut approval: Option<(String, u128)> = None;
        let mut extra =
            json!({"finance_module":key,"registry_id":agent_key,"input_hash":input_hash});
        let ix = match input.action.as_str() {
            "pool_deposit" | "stock_deposit" => {
                let amount = positive(input.amount, 18)?;
                if amount
                    > bnb::number(
                        &self
                            .finance_view(&module, "maxDeposit", vec![bnb::addr(&wallet)?])
                            .await?,
                    )?
                {
                    return Err(ApiError::validation("Deposit exceeds the pool's maximum."));
                }
                approval = Some((bnb::USDT.into(), amount));
                instruction(
                    &module,
                    "deposit",
                    vec![bnb::uint(amount), bnb::addr(&wallet)?],
                )?
            }
            "pool_redeem" | "stock_redeem" => {
                let shares = positive(input.amount, 18)?;
                if shares
                    > bnb::number(
                        &self
                            .finance_view(&module, "maxRedeem", vec![bnb::addr(&wallet)?])
                            .await?,
                    )?
                {
                    return Err(ApiError::validation(
                        "These shares exceed your balance or the pool's available liquidity.",
                    ));
                }
                instruction(
                    &module,
                    "redeem",
                    vec![bnb::uint(shares), bnb::addr(&wallet)?, bnb::addr(&wallet)?],
                )?
            }
            "advance_approve" => {
                if module.summary["underwriter"] != wallet {
                    return Err(ApiError::forbidden(
                        "Only the verified pool underwriter approves working-capital risk.",
                    ));
                }
                schema(&self.store)?;
                let rid = required(
                    &input.request_id,
                    "Choose an existing working-capital request.",
                )?;
                let row: Option<(String, String)> = self
                    .store
                    .connect()?
                    .query_row(
                        "SELECT owner,payload FROM finance_requests WHERE id=?",
                        [rid],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                let (borrower_owner, raw) =
                    row.ok_or_else(|| ApiError::missing("Working-capital request not found."))?;
                let request: FinanceRequest = decode(raw)?;
                let borrower = self.store.agent(&borrower_owner, &request.agent_id)?;
                if input
                    .target_agent_id
                    .as_ref()
                    .is_some_and(|target| target != &request.agent_id)
                    || request.terms_hash
                        != Store::job(&self.store.connect()?, &request.job_id)?.terms_hash
                {
                    return Err(ApiError::conflict("Working-capital request terms changed."));
                }
                input.loan_id = Some(request.loan_id);
                input.target_agent_id = Some(request.agent_id);
                input.job_id = Some(request.job_id);
                input.amount = request.amount;
                input.per_call = Some(request.per_call);
                input.daily_cap = Some(request.daily_cap);
                input.expires_at = Some(request.expires_at);
                input.signer = Some(request.signer);
                input.tools = request.tools;
                input.recipients = request.recipients;
                let (actual_wallet, job, _) = self
                    .finance_request_terms(&borrower, &input, &module)
                    .await?;
                if actual_wallet != request.borrower {
                    return Err(ApiError::conflict(
                        "The requested borrower's wallet changed.",
                    ));
                }
                let borrower_key = borrower.registry_id.as_deref().expect("verified borrower");
                extra["registry_id"] = json!(borrower_key);
                extra["job_onchain_id"] = json!(self.bnb.job_address(&job.id)?);
                let principal = positive(input.amount, 18)?;
                if principal
                    > bnb::number(
                        &self
                            .finance_view(&module, "availableLiquidity", vec![])
                            .await?,
                    )?
                {
                    return Err(ApiError::conflict(
                        "The pool does not have enough unreserved USDT.",
                    ));
                }
                instruction(
                    &module,
                    "approveLoan",
                    vec![Token::Tuple(vec![
                        nonzero_hash(input.loan_id.as_deref().expect("request"))?,
                        bnb::bytes32(borrower_key)?,
                        bnb::bytes32(&job.id)?,
                        bnb::addr(input.signer.as_deref().expect("validated"))?,
                        bnb::uint(principal),
                        bnb::uint(units(input.per_call.expect("validated"))?),
                        bnb::uint(units(input.daily_cap.expect("validated"))?),
                        bnb::uint(input.expires_at.expect("validated") as u128),
                        bnb::uint(tool_bitmap(&input.tools).into()),
                        Token::Array(
                            input
                                .recipients
                                .iter()
                                .map(|r| bnb::addr(r))
                                .collect::<Result<Vec<_>>>()?,
                        ),
                    ])],
                )?
            }
            "advance_accept" | "advance_spend" | "advance_repay" | "advance_close" => {
                let loan_id = bnb::id(required(
                    &input.loan_id,
                    "Choose the exact working-capital line.",
                )?)?;
                input.loan_id = Some(loan_id.clone());
                let loan = self
                    .finance_view(&module, "getLoan", vec![nonzero_hash(&loan_id)?])
                    .await?;
                if loan["borrower"] == bnb::ZERO {
                    return Err(ApiError::missing("Working-capital line not found."));
                }
                let borrower = loan["borrower"] == wallet && loan["agent"] == agent_key;
                if !borrower
                    && !(input.action == "advance_close" && module.summary["underwriter"] == wallet)
                {
                    return Err(ApiError::forbidden(
                        "This working-capital line belongs to another registered agent.",
                    ));
                }
                let mut args = vec![bnb::bytes32(&loan_id)?];
                let method = match input.action.as_str() {
                    "advance_accept" => {
                        if loan["accepted"] == true || loan["closed"] == true {
                            return Err(ApiError::conflict(
                                "This line is already accepted or closed.",
                            ));
                        }
                        "acceptLoan"
                    }
                    "advance_close" => "closeLoan",
                    "advance_repay" => {
                        let amount = positive(input.amount, 18)?;
                        if amount > bnb::number(&loan["debt"])? {
                            return Err(ApiError::validation(
                                "Repayment exceeds outstanding principal.",
                            ));
                        }
                        approval = Some((bnb::USDT.into(), amount));
                        args.push(bnb::uint(amount));
                        "repayLoan"
                    }
                    _ => {
                        let amount = positive(input.amount, 18)?;
                        let recipient = bnb::address(required(
                            &input.recipient,
                            "Choose an approved merchant recipient.",
                        )?)?;
                        let tool = required(&input.tool, "Choose the exact paid tool.")?;
                        if !agent.plan.tools.iter().any(|t| t == tool)
                            || !TOOLS.contains(&tool)
                            || amount > bnb::number(&loan["perCall"])?
                            || amount > bnb::number(&loan["available"])?
                            || !loan["recipients"].as_array().is_some_and(|recipients| {
                                recipients.iter().any(|r| r == &recipient)
                            })
                            || (u128::from(tool_bitmap(&[tool.into()]))
                                & !bnb::number(&loan["tools"])?)
                                != 0
                            || !self.merchants()?.iter().any(|m| {
                                m.tool == tool && m.recipient.eq_ignore_ascii_case(&recipient)
                            })
                        {
                            return Err(ApiError::validation("Spend within this line's exact tool, merchant, and per-call limit."));
                        }
                        input.recipient = Some(recipient.clone());
                        args.extend([
                            bnb::addr(&recipient)?,
                            bnb::uint(amount),
                            bnb::uint(tool_bitmap(&[tool.into()]).into()),
                            nonzero_hash(required(
                                &input.request_hash,
                                "Commit the merchant request.",
                            )?)?,
                            nonzero_hash(required(
                                &input.receipt_hash,
                                "Commit the expected result receipt.",
                            )?)?,
                        ]);
                        "spendLoan"
                    }
                };
                instruction(&module, method, args)?
            }
            "stock_borrow" => {
                if module.summary["paused"] == true {
                    return Err(ApiError::conflict("New collateral borrowing is paused."));
                }
                let token = bnb::address(required(
                    &input.token_address,
                    "Choose the verified collateral token.",
                )?)?;
                let (asset, _) = self.finance_asset(&module, &token).await?;
                let collateral = positive(
                    input
                        .collateral_amount
                        .ok_or_else(|| ApiError::validation("Set the collateral amount."))?,
                    asset.decimals,
                )?;
                let principal = positive(input.amount, 18)?;
                let q = self
                    .finance_view(
                        &module,
                        "collateralQuote",
                        vec![bnb::addr(&token)?, bnb::uint(collateral)],
                    )
                    .await?;
                if principal > bnb::number(&q[1])?
                    || principal
                        > bnb::number(
                            &self
                                .finance_view(&module, "availableLiquidity", vec![])
                                .await?,
                        )?
                {
                    return Err(ApiError::validation(
                        "The principal exceeds fresh borrowing power or available pool liquidity.",
                    ));
                }
                let loan_id = if let Some(id) = &input.loan_id {
                    bnb::id(id)?
                } else {
                    bnb::hash(format!("tab-stock-loan:{}", identifier()).as_bytes())
                };
                if self
                    .finance_view(&module, "getLoan", vec![nonzero_hash(&loan_id)?])
                    .await?["borrower"]
                    != bnb::ZERO
                {
                    return Err(ApiError::conflict(
                        "This collateral loan identifier is already used.",
                    ));
                }
                input.loan_id = Some(loan_id.clone());
                input.token_address = Some(token.clone());
                extra["token_decimals"] = json!(asset.decimals);
                approval = Some((token.clone(), collateral));
                instruction(
                    &module,
                    "borrow",
                    vec![
                        bnb::bytes32(&loan_id)?,
                        bnb::addr(&token)?,
                        bnb::uint(collateral),
                        bnb::uint(principal),
                    ],
                )?
            }
            "stock_add_collateral" | "stock_withdraw" | "stock_repay" | "stock_liquidate" => {
                let loan_id = bnb::id(required(
                    &input.loan_id,
                    "Choose the exact collateral loan.",
                )?)?;
                input.loan_id = Some(loan_id.clone());
                let loan = self
                    .finance_view(&module, "getLoan", vec![nonzero_hash(&loan_id)?])
                    .await?;
                if loan["borrower"] == bnb::ZERO {
                    return Err(ApiError::missing("Collateral loan not found."));
                }
                if input.action != "stock_liquidate" && loan["borrower"] != wallet {
                    return Err(ApiError::forbidden(
                        "This collateral loan belongs to another wallet.",
                    ));
                }
                let token = loan["collateralToken"]
                    .as_str()
                    .ok_or_else(ApiError::internal)?;
                let decimals: u8 = bnb::number(&loan["terms"]["tokenDecimals"])?
                    .try_into()
                    .map_err(|_| ApiError::unavailable("Invalid loan token precision."))?;
                if input.action == "stock_add_collateral" {
                    self.finance_collateral_token(token, None).await?;
                    if self.bnb.token_decimals(token).await? != decimals {
                        return Err(ApiError::unavailable("Collateral token precision changed."));
                    }
                }
                if input.action == "stock_liquidate"
                    || (input.action == "stock_withdraw" && bnb::number(&loan["debt"])? > 0)
                {
                    self.finance_historical_asset(&module, &loan).await?;
                }
                if input
                    .token_address
                    .as_ref()
                    .is_some_and(|t| !t.eq_ignore_ascii_case(token))
                {
                    return Err(ApiError::validation(
                        "Use this loan's exact collateral token.",
                    ));
                }
                input.token_address = Some(token.into());
                extra["token_decimals"] = json!(decimals);
                let mut args = vec![bnb::bytes32(&loan_id)?];
                let method = match input.action.as_str() {
                    "stock_add_collateral" | "stock_withdraw" => {
                        let amount =
                            positive(input.collateral_amount.unwrap_or(input.amount), decimals)?;
                        input.collateral_amount = Some(Decimal::from_i128_with_scale(
                            amount as i128,
                            decimals.into(),
                        ));
                        if input.action == "stock_add_collateral" {
                            approval = Some((token.into(), amount));
                            "addCollateral"
                        } else {
                            if amount > bnb::number(&loan["collateral"])? {
                                return Err(ApiError::validation(
                                    "Withdrawal exceeds the pledged collateral.",
                                ));
                            }
                            "withdrawCollateral"
                        }
                    }
                    "stock_repay" => {
                        let amount = positive(input.amount, 18)?;
                        if amount > bnb::number(&loan["debt"])? {
                            return Err(ApiError::validation(
                                "Repayment exceeds outstanding principal.",
                            ));
                        }
                        approval = Some((bnb::USDT.into(), amount));
                        "repay"
                    }
                    _ => {
                        let amount = positive(input.amount, 18)?;
                        let quote = self
                            .finance_view(
                                &module,
                                "liquidationQuote",
                                vec![bnb::bytes32(&loan_id)?, bnb::uint(amount)],
                            )
                            .await?;
                        let seize = bnb::number(&quote[1])?;
                        let minimum = if let Some(minimum) = input.minimum_out {
                            positive(minimum, decimals)?
                        } else {
                            seize
                        };
                        if minimum == 0 || minimum < seize {
                            return Err(ApiError::validation("Keep the minimum collateral output at the verified liquidation quote."));
                        }
                        input.minimum_out = Some(Decimal::from_i128_with_scale(
                            minimum as i128,
                            decimals.into(),
                        ));
                        approval = Some((bnb::USDT.into(), amount));
                        args.extend([bnb::uint(amount), bnb::uint(minimum)]);
                        "liquidate"
                    }
                };
                if input.action != "stock_liquidate" {
                    args.push(bnb::uint(if input.action == "stock_repay" {
                        positive(input.amount, 18)?
                    } else {
                        positive(input.collateral_amount.expect("normalized"), decimals)?
                    }));
                }
                instruction(&module, method, args)?
            }
            "buyback_fund" => {
                let amount = positive(input.amount, 18)?;
                if input.amount > Decimal::from(10000) {
                    return Err(ApiError::validation(
                        "Keep a buyback contribution within 10,000 USDT.",
                    ));
                }
                approval = Some((bnb::USDT.into(), amount));
                instruction(&module, "fund", vec![bnb::uint(amount)])?
            }
            "buyback_execute" => {
                if module.summary["operator"] != wallet {
                    return Err(ApiError::forbidden(
                        "Only the verified buyback operator can execute this route.",
                    ));
                }
                if module.summary["paused"] == true {
                    return Err(ApiError::conflict("Buybacks are paused."));
                }
                let amount = positive(input.amount, 18)?;
                let q = self
                    .finance_view(&module, "quote", vec![bnb::uint(amount)])
                    .await?;
                let token = module.summary["token"]
                    .as_str()
                    .ok_or_else(ApiError::internal)?;
                let decimals = self.bnb.token_decimals(token).await?;
                let minimum = if let Some(minimum) = input.minimum_out {
                    positive(minimum, decimals)?
                } else {
                    bnb::number(&q[1])?
                };
                if minimum < bnb::number(&q[1])? {
                    return Err(ApiError::validation(
                        "Minimum TAB output exceeds the allowed slippage.",
                    ));
                }
                let now = Utc::now().timestamp();
                let deadline = input.deadline.unwrap_or(now + 600);
                if deadline <= now || deadline > now + 600 {
                    return Err(ApiError::validation(
                        "Buyback execution expires within ten minutes.",
                    ));
                }
                input.minimum_out = Some(Decimal::from_i128_with_scale(
                    minimum as i128,
                    decimals.into(),
                ));
                input.deadline = Some(deadline);
                extra["official_token"] = json!(token);
                extra["token_decimals"] = json!(decimals);
                instruction(
                    &module,
                    "execute",
                    vec![
                        bnb::uint(amount),
                        bnb::uint(minimum),
                        bnb::uint(deadline as u128),
                    ],
                )?
            }
            _ => return Err(ApiError::validation("Choose a supported finance action.")),
        };
        let transactions = self
            .bnb
            .transactions(
                &wallet,
                &ix,
                approval.as_ref().map(|(t, a)| (t.as_str(), *a)),
            )
            .await?;
        let mut details = serde_json::to_value(&input)?;
        details
            .as_object_mut()
            .expect("serialized struct")
            .extend(extra.as_object().expect("metadata").clone());
        if let Some((token, amount)) = approval {
            details["approval_token"] = json!(token);
            details["approval_amount_raw"] = json!(amount.to_string());
        }
        let intent = TransactionIntent {
            tx_hash: None,
            id: identifier(),
            agent_id: id.into(),
            action,
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
    pub async fn finance_verify_intent(&self, intent: &TransactionIntent) -> Result<()> {
        let action = intent
            .action
            .strip_prefix("finance_")
            .ok_or_else(|| ApiError::validation("This is not a finance intent."))?;
        let key = module_for_action(action)?;
        if intent.details["finance_module"] != key
            || intent.details["action"] != action
            || intent.chain_id != 56
        {
            return Err(ApiError::unavailable("Finance intent scope is invalid."));
        }
        let module = self.finance_module(key).await?;
        if intent.to != module.address || intent.transaction.to != module.address {
            return Err(ApiError::unavailable(
                "Finance deployment changed after preparation.",
            ));
        }
        if key == "buyback"
            && intent.details["official_token"].is_string()
            && intent.details["official_token"] != module.summary["token"]
        {
            return Err(ApiError::unavailable("The official buyback token changed."));
        }
        Ok(())
    }
    pub async fn finance_record_confirmation(&self, intent: &TransactionIntent) -> Result<()> {
        let action = intent.action.strip_prefix("finance_").unwrap_or_default();
        if !action.starts_with("advance_") {
            return Ok(());
        }
        let loan_id = intent.details["loan_id"]
            .as_str()
            .ok_or_else(ApiError::internal)?;
        schema(&self.store)?;
        let raw: Option<String> = self
            .store
            .connect()?
            .query_row(
                "SELECT payload FROM finance_requests WHERE json_extract(payload,'$.loan_id')=?",
                [loan_id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(raw) = raw else {
            return Ok(());
        };
        let mut request: FinanceRequest = decode(raw)?;
        let module = self.finance_module("lending").await?;
        let loan = self
            .finance_view(&module, "getLoan", vec![bnb::bytes32(loan_id)?])
            .await?;
        if loan["borrower"] != request.borrower
            || loan["job"] != self.bnb.job_address(&request.job_id)?
            || loan["agent"]
                != self
                    .bnb
                    .agent_address(&request.borrower, &request.agent_id)?
            || bnb::number(&loan["principal"])? != units(request.amount)?
        {
            return Err(ApiError::unavailable(
                "Confirmed working-capital terms differ from the immutable request.",
            ));
        }
        request.status = if loan["closed"] == true {
            "closed"
        } else if loan["accepted"] == true {
            "accepted"
        } else {
            "approved"
        }
        .into();
        self.store.connect()?.execute(
            "UPDATE finance_requests SET payload=? WHERE id=?",
            params![encode(&request)?, request.id],
        )?;
        Ok(())
    }
    async fn finance_pool_account(
        &self,
        module: &VerifiedModule,
        wallet: &str,
        agent_key: &str,
        ids: &BTreeSet<String>,
        underwriter: bool,
    ) -> Result<Value> {
        let (shares, max_redeem) = tokio::try_join!(
            self.finance_view(module, "balanceOf", vec![bnb::addr(wallet)?]),
            self.finance_view(module, "maxRedeem", vec![bnb::addr(wallet)?])
        )?;
        let shares = bnb::number(&shares)?;
        let max_redeem = bnb::number(&max_redeem)?;
        let share_value = self
            .finance_view(module, "previewRedeem", vec![bnb::uint(shares)])
            .await?;
        let mut rows = vec![];
        for id in ids.iter().take(50) {
            let loan = self
                .finance_view(module, "getLoan", vec![bnb::bytes32(id)?])
                .await?;
            if loan["borrower"] == bnb::ZERO {
                continue;
            }
            if module.key == "lending" {
                let borrower = loan["borrower"] == wallet && loan["agent"] == agent_key;
                if !borrower && !underwriter {
                    continue;
                }
                let expires = bnb::number(&loan["expiresAt"])?;
                let available = bnb::number(&loan["available"])?;
                let debt = bnb::number(&loan["debt"])?;
                let active = module.summary["paused"] != true
                    && loan["closed"] != true
                    && expires > Utc::now().timestamp() as u128;
                let tools = bnb::number(&loan["tools"])?;
                rows.push(json!({"id":id,"agent":loan["agent"],"job":loan["job"],"borrower":loan["borrower"],"signer":loan["signer"],
                    "principal":money(bnb::number(&loan["principal"])?).to_string(),"available":money(available).to_string(),"debt":money(debt).to_string(),"loss":money(bnb::number(&loan["loss"])?).to_string(),
                    "spent":money(bnb::number(&loan["spent"])?).to_string(),"repaid":money(bnb::number(&loan["repaid"])?).to_string(),"per_call":money(bnb::number(&loan["perCall"])?).to_string(),"daily_cap":money(bnb::number(&loan["dailyCap"])?).to_string(),
                    "expires_at":expires as u64,"tools":TOOLS.iter().enumerate().filter(|(i,_)|tools&(1<<i)!=0).map(|(_,t)|*t).collect::<Vec<_>>(),"recipients":loan["recipients"],"accepted":loan["accepted"],"closed":loan["closed"],
                    "actions":{"accept":borrower&&active&&loan["accepted"]!=true,"spend":borrower&&active&&loan["accepted"]==true&&available>0,"repay":borrower&&debt>0,"close":(borrower||underwriter)&&loan["closed"]!=true}}));
            } else {
                // IDs collected from owned receipts are still checked against
                // the chain borrower before any private account row is shown.
                if loan["borrower"] != wallet {
                    continue;
                }
                let token = loan["collateralToken"]
                    .as_str()
                    .ok_or_else(ApiError::internal)?;
                let decimals: u8 = bnb::number(&loan["terms"]["tokenDecimals"])?
                    .try_into()
                    .map_err(|_| ApiError::unavailable("Invalid collateral token precision."))?;
                if decimals > 18 {
                    return Err(ApiError::unavailable(
                        "Unsupported collateral token precision.",
                    ));
                }
                let health = if self.finance_historical_asset(module, &loan).await.is_ok() {
                    self.finance_view(module, "loanHealth", vec![bnb::bytes32(id)?])
                        .await
                        .ok()
                } else {
                    None
                };
                let collateral = bnb::number(&loan["collateral"])?;
                let debt = bnb::number(&loan["debt"])?;
                let symbol = module
                    .config
                    .assets
                    .iter()
                    .find(|a| a.token.eq_ignore_ascii_case(token))
                    .map(|a| a.symbol.as_str())
                    .unwrap_or("collateral");
                let maximum = health.as_ref().and_then(|h| bnb::number(&h[1]).ok());
                rows.push(json!({"id":id,"borrower":wallet,"token":token,"symbol":symbol,"decimals":decimals,"collateral":decimal_units(collateral,decimals),"debt":money(debt).to_string(),"loss":money(bnb::number(&loan["loss"])?).to_string(),
                    "collateral_value_usdt":health.as_ref().and_then(|h|bnb::number(&h[0]).ok()).map(|n|money(n).to_string()),"maximum_borrow_usdt":maximum.map(|n|money(n).to_string()),"liquidation_debt_usdt":health.as_ref().and_then(|h|bnb::number(&h[2]).ok()).map(|n|money(n).to_string()),
                    "liquidatable":health.as_ref().is_some_and(|h|h[3]==true),"oracle_status":if health.is_some(){"verified"}else{"unavailable"},
                    "actions":{"add_collateral":debt>0,"withdraw":collateral>0&&(debt==0||(module.summary["paused"]!=true&&maximum.is_some_and(|v|v>debt))),"repay":debt>0,"liquidate":health.as_ref().is_some_and(|h|h[3]==true)}}));
            }
        }
        Ok(
            json!({"shares":money(shares).to_string(),"max_redeem":money(max_redeem).to_string(),"share_value_usdt":money(bnb::number(&share_value)?).to_string(),"loans":rows,"positions_truncated":ids.len()>50}),
        )
    }
    pub async fn finance_account(&self, owner: &str, id: &str) -> Result<Value> {
        let agent = self.store.agent(owner, id)?;
        let system = self.finance_system().await;
        let pending_intents = self.wallet_actions(owner, id)?;
        schema(&self.store)?;
        let requests = self.finance_requests(owner, id).await?;
        let mut account = json!({"system":system,"wallet":null,"status":"not_registered","lending":{"shares":null,"max_redeem":null,"share_value_usdt":null,"loans":[]},"stock_loans":{"shares":null,"max_redeem":null,"share_value_usdt":null,"loans":[]},"requests":requests["requests"],"pending_intents":pending_intents,"pending_job_intents":[],"roles":{"underwriter":false,"buyback_operator":false}});
        if agent.registry_id.is_none() {
            return Ok(account);
        }
        let wallet = self.verify_owned_agent(&agent).await?;
        let agent_key = agent.registry_id.as_deref().expect("verified agent");
        account["wallet"] = json!(wallet);
        account["status"] = json!("ready");
        account["pending_intents"]=json!(Store::list_payload::<TransactionIntent,_>(&self.store.connect()?,"SELECT json_set(payload,'$.tx_hash',tx_hash) FROM wallet_intents WHERE owner=? AND lower(json_extract(payload,'$.sender'))=lower(?) AND confirmed=0 AND (tx_hash IS NOT NULL OR julianday(json_extract(payload,'$.expires_at'))>julianday(?))",params![owner,wallet,now()])?);
        account["pending_job_intents"] = json!(self.finance_pending_jobs(owner, &wallet)?);
        let confirmed:Vec<TransactionIntent>=Store::list_payload(&self.store.connect()?,"SELECT payload FROM wallet_intents WHERE owner=? AND confirmed=1 AND json_extract(payload,'$.details.finance_module') IS NOT NULL",[owner])?;
        for key in ["lending", "stock_loans"] {
            if account["system"]["modules"][key]["status"] != "verified" {
                continue;
            }
            let module = self.finance_module(key).await?;
            let underwriter = module.summary["underwriter"] == wallet;
            if underwriter {
                account["roles"]["underwriter"] = json!(true);
            }
            let mut ids: BTreeSet<String> = confirmed
                .iter()
                .filter(|i| {
                    i.details["finance_module"] == key && i.sender.eq_ignore_ascii_case(&wallet)
                })
                .filter_map(|i| i.details["loan_id"].as_str().map(str::to_owned))
                .collect();
            if key == "lending" {
                for request in requests["requests"].as_array().into_iter().flatten() {
                    if (request["agent_id"] == id || underwriter) && request["loan_id"].is_string()
                    {
                        ids.insert(request["loan_id"].as_str().expect("string").into());
                    }
                }
            }
            account[key] = self
                .finance_pool_account(&module, &wallet, agent_key, &ids, underwriter)
                .await?;
        }
        account["roles"]["buyback_operator"] = json!(
            account["system"]["modules"]["buyback"]["status"] == "verified"
                && account["system"]["modules"]["buyback"]["operator"] == wallet
        );
        Ok(account)
    }
}
