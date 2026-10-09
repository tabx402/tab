//! Chain boundary regression tests: these use an in-process JSON-RPC fixture, never funded wallets.
use crate::{
    bnb::{self, Instruction},
    config::Config,
    models::*,
    AppState,
};
use axum::{extract::State, routing::post, Json, Router};
use ethabi::{ethereum_types::U256, Token};
use k256::ecdsa::SigningKey;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex},
};
const PROTOCOL: &str = "0x1111111111111111111111111111111111111111";
const BACKING: &str = "0x2222222222222222222222222222222222222222";
const ECONOMICS: &str = "0x3333333333333333333333333333333333333333";
const WALLET: &str = "0x4444444444444444444444444444444444444444";
const TXHASH: &str = "0x5555555555555555555555555555555555555555555555555555555555555555";
const BLOCK: &str = "0x6666666666666666666666666666666666666666666666666666666666666666";
type Replies = Arc<Mutex<HashMap<String, Value>>>;
async fn rpc(State(replies): State<Replies>, Json(request): Json<Value>) -> Json<Value> {
    let method = request["method"].as_str().unwrap();
    let delay = replies
        .lock()
        .unwrap()
        .get(&format!("delay:{method}"))
        .and_then(Value::as_u64);
    if let Some(delay) = delay {
        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
    }
    let key = if method == "eth_call" {
        format!(
            "call:{}",
            request["params"][0]["data"]
                .as_str()
                .unwrap()
                .get(..10)
                .unwrap_or("")
        )
    } else {
        method.into()
    };
    let mut values = replies.lock().unwrap();
    values.insert(format!("request:{key}"), request.clone());
    if method == "eth_sendRawTransaction" {
        values
            .entry("sent".into())
            .or_insert(json!([]))
            .as_array_mut()
            .unwrap()
            .push(request["params"][0].clone());
        let hash = bnb::hash(
            &hex::decode(
                request["params"][0]
                    .as_str()
                    .unwrap()
                    .trim_start_matches("0x"),
            )
            .unwrap(),
        );
        return Json(if values.get("broadcast_error") == Some(&json!(true)) {
            json!({"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"fixture delivery timeout"}})
        } else {
            json!({"jsonrpc":"2.0","id":1,"result":hash})
        });
    }
    let result = if method == "eth_getCode" {
        values
            .get(&format!("code:{}", request["params"][0].as_str().unwrap()))
            .or_else(|| values.get(&key))
            .cloned()
    } else {
        values.get(&key).cloned()
    };
    Json(match result {
        Some(result) => json!({"jsonrpc":"2.0","id":request["id"],"result":result}),
        None => {
            json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32601,"message":"unconfigured fixture"}})
        }
    })
}
fn abi() -> ethabi::Contract {
    ethabi::Contract::load(include_bytes!("../../contracts/bnb/abi/TabProtocol.json").as_slice())
        .unwrap()
}
fn selector(signature: &str) -> String {
    format!(
        "0x{}",
        hex::encode(&Keccak256::digest(signature.as_bytes())[..4])
    )
}
fn encoded(tokens: &[Token]) -> Value {
    json!(format!("0x{}", hex::encode(ethabi::encode(tokens))))
}
fn uint(n: u128) -> Token {
    bnb::uint(n)
}
fn protocol() -> Value {
    encoded(&[Token::Tuple(vec![
        bnb::addr(WALLET).unwrap(),
        bnb::addr(bnb::USDT).unwrap(),
        bnb::addr(bnb::ZERO).unwrap(),
        bnb::addr(BACKING).unwrap(),
        bnb::addr(ECONOMICS).unwrap(),
        uint(200),
        uint(0),
        uint(0),
        uint(0),
        uint(0),
    ])])
}
pub(crate) struct Fixture {
    _dir: tempfile::TempDir,
    pub(crate) state: AppState,
    pub(crate) replies: Replies,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
pub(crate) async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let replies: Replies = Arc::new(Mutex::new(HashMap::from([
        ("eth_chainId".into(), json!("0x38")),
        ("eth_getCode".into(), json!("0x60006000")),
        ("eth_blockNumber".into(), json!("0x66")),
        ("eth_estimateGas".into(), json!("0x186a0")),
        ("eth_getLogs".into(), json!([])),
        ("eth_getBlockByNumber".into(), json!({"hash":BLOCK})),
        (
            "eth_getTransactionReceipt".into(),
            json!({"transactionHash":TXHASH,"status":"0x1","blockNumber":"0x64","blockHash":BLOCK,"to":PROTOCOL,"from":WALLET,"logs":[]}),
        ),
        (
            "eth_getTransactionByHash".into(),
            json!({"hash":TXHASH,"chainId":"0x38","from":WALLET,"to":PROTOCOL,"input":"0x1234","value":"0x0","blockNumber":"0x64","blockHash":BLOCK}),
        ),
        (
            format!("call:{}", selector("decimals()")),
            encoded(&[uint(18)]),
        ),
        (format!("call:{}", selector("getProtocol()")), protocol()),
        (
            format!("call:{}", selector("protocol()")),
            encoded(&[bnb::addr(PROTOCOL).unwrap()]),
        ),
        (
            format!("call:{}", selector("usdt()")),
            encoded(&[bnb::addr(bnb::USDT).unwrap()]),
        ),
        (
            format!("call:{}", selector("allowance(address,address)")),
            encoded(&[uint(0)]),
        ),
    ])));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let rpc_url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/", post(rpc))
        .with_state(replies.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let hash = bnb::hash(&hex::decode("60006000").unwrap());
    let manifest = root.join("manifest.json");
    std::fs::write(&manifest,serde_json::to_vec(&json!({"chain_id":56,"authority":WALLET,"official_tab_address":null,"usdt_address":bnb::USDT,"usdt_decimals":18,"usdt_code_hash":hash,"deployment_block":1,"contracts":{"protocol":{"address":PROTOCOL,"code_hash":hash},"backing":{"address":BACKING,"code_hash":hash},"economics":{"address":ECONOMICS,"code_hash":hash}}})).unwrap()).unwrap();
    let config = Config {
        sponsor_enabled: false,
        sponsor_key: None,
        sponsor_address: None,
        network: "mainnet".into(),
        chain_id: 56,
        rpc: rpc_url,
        logs_rpc: None,
        usdt: bnb::USDT.into(),
        program: PROTOCOL.into(),
        official_tab: None,
        app_id: None,
        database: root.join("bnb.sqlite"),
        manifest,
        merchants: root.join("merchants.json"),
        x402_merchants: root.join("x402.json"),
        bind: "127.0.0.1:0".into(),
        openrouter_key: None,
        tavily_key: None,
        inference_daily_micros: 100000,
        confirmations: 3,
    };
    Fixture {
        state: AppState::new(config).unwrap(),
        _dir: dir,
        replies,
        server,
    }
}
fn ix() -> Instruction {
    Instruction {
        to: PROTOCOL.into(),
        data: "0x1234".into(),
        value: "0x0".into(),
    }
}
#[tokio::test]
async fn final_receipt_binds_chain_signer_destination_value_and_calldata() {
    let f = fixture().await;
    assert!(f
        .state
        .bnb
        .verify_transaction(TXHASH, WALLET, &ix())
        .await
        .is_ok());
    let baseline = f.replies.lock().unwrap()["eth_getTransactionByHash"].clone();
    for (field, bad) in [
        ("chainId", json!("0x1")),
        ("from", json!(BACKING)),
        ("to", json!(ECONOMICS)),
        ("input", json!("0x1235")),
        ("value", json!("0x1")),
        ("blockHash", json!(TXHASH)),
        ("blockNumber", json!("0x63")),
        ("hash", json!(BLOCK)),
    ] {
        let mut tx = baseline.clone();
        tx[field] = bad;
        f.replies
            .lock()
            .unwrap()
            .insert("eth_getTransactionByHash".into(), tx);
        assert!(
            f.state
                .bnb
                .verify_transaction(TXHASH, WALLET, &ix())
                .await
                .is_err(),
            "accepted altered {field}"
        );
    }
}
#[tokio::test]
async fn finality_reverts_reorgs_and_wrong_deployment_fail_closed() {
    let f = fixture().await;
    let original = f.replies.lock().unwrap().clone();
    for (key, value) in [
        ("eth_chainId", json!("0x1")),
        ("eth_getCode", json!("0x6001")),
        ("eth_blockNumber", json!("0x64")),
        ("eth_getBlockByNumber", json!({"hash":TXHASH})),
        ("eth_getTransactionReceipt", Value::Null),
    ] {
        *f.replies.lock().unwrap() = original.clone();
        f.replies.lock().unwrap().insert(key.into(), value);
        assert!(
            f.state
                .bnb
                .verify_transaction(TXHASH, WALLET, &ix())
                .await
                .is_err(),
            "accepted altered {key}"
        );
    }
    *f.replies.lock().unwrap() = original;
    let mut receipt = f.replies.lock().unwrap()["eth_getTransactionReceipt"].clone();
    receipt["status"] = json!("0x0");
    f.replies
        .lock()
        .unwrap()
        .insert("eth_getTransactionReceipt".into(), receipt);
    assert!(f
        .state
        .bnb
        .verify_transaction(TXHASH, WALLET, &ix())
        .await
        .is_err());
}
#[tokio::test]
async fn approval_is_exact_and_resets_existing_allowance_before_action() {
    let f = fixture().await;
    let amount = 1000000000000000001;
    let txs = f
        .state
        .bnb
        .transactions(WALLET, &ix(), Some((bnb::USDT, amount)))
        .await
        .unwrap();
    assert_eq!(txs.len(), 2);
    let approve = hex::decode(txs[0].data.trim_start_matches("0x")).unwrap();
    let decoded = ethabi::decode(
        &[ethabi::ParamType::Address, ethabi::ParamType::Uint(256)],
        &approve[4..],
    )
    .unwrap();
    assert_eq!(decoded[0], bnb::addr(PROTOCOL).unwrap());
    assert_eq!(decoded[1], uint(amount));
    assert_eq!(txs[0].chain_id, "0x38");
    f.replies.lock().unwrap().insert(
        format!("call:{}", selector("allowance(address,address)")),
        encoded(&[uint(1)]),
    );
    let txs = f
        .state
        .bnb
        .transactions(WALLET, &ix(), Some((bnb::USDT, amount)))
        .await
        .unwrap();
    assert_eq!(txs.len(), 3);
    assert!(txs[0].data.ends_with(&"0".repeat(64)));
    assert_eq!(txs[2].data, "0x1234");
    f.replies.lock().unwrap().insert(
        format!("call:{}", selector("allowance(address,address)")),
        encoded(&[Token::Uint(U256::MAX)]),
    );
    assert_eq!(
        f.state
            .bnb
            .transactions(WALLET, &ix(), Some((bnb::USDT, amount)))
            .await
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn usdt_precision_and_inference_metering_have_distinct_units() {
    let amount = rust_decimal::Decimal::from_str("0.123456789012345678").unwrap();
    assert_eq!(units(amount).unwrap(), 123456789012345678);
    assert_eq!(money(units(amount).unwrap()), amount);
    assert_eq!(usd_micros(amount).unwrap(), 123456);
    assert!(units(rust_decimal::Decimal::from_str("0.0000000000000000001").unwrap()).is_err());
    let p = json!({"name":"agent","purpose":"test exact values","tools":["bnb-rpc"],"daily_cap":"10","max_call":0.1});
    assert!(serde_json::from_value::<AgentInput>(p).is_err());
    assert!(serde_json::from_value::<FinancialInput>(
        json!({"action":"fund_spending","amount":0.1})
    )
    .is_err());
}
#[test]
fn database_cannot_mix_chain_history_or_reuse_financial_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("CREATE TABLE runtime_agents(id TEXT)", [])
        .unwrap();
    drop(db);
    assert!(crate::db::Store::open(&path).is_err());
    let store = crate::db::Store::open(&dir.path().join("new.sqlite")).unwrap();
    store.claim_receipt(TXHASH, "one").unwrap();
    store.claim_receipt(TXHASH, "one").unwrap();
    assert!(store.claim_receipt(TXHASH, "two").is_err());
    assert!(store.claim_receipt(BLOCK, "one").is_err());
}
async fn registration_case(expired: bool) {
    let f = fixture().await;
    let key = SigningKey::from_bytes((&[7u8; 32]).into()).unwrap();
    let point = key.verifying_key().to_encoded_point(false);
    let wallet = format!(
        "0x{}",
        hex::encode(&Keccak256::digest(&point.as_bytes()[1..])[12..])
    );
    let agent=f.state.create_agent("owner",serde_json::from_value(json!({"name":"willow","purpose":"check primary sources","daily_cap":"10","max_call":"0.1","tools":["bnb-rpc"]})).unwrap()).unwrap();
    let address = f.state.bnb.agent_address(&wallet, &agent.id).unwrap();
    assert!(address.starts_with(&wallet));
    assert_eq!(address.len(), 66);
    let challenge = f
        .state
        .challenge("owner", &agent.id, &wallet)
        .await
        .unwrap();
    let original = challenge["message"].as_str().unwrap();
    let expired_message;
    let message = if expired {
        let expires = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        expired_message = format!(
            "{}\nExpires {}",
            original.rsplit_once("\nExpires ").unwrap().0,
            expires
        );
        f.state
            .store
            .connect()
            .unwrap()
            .execute(
                "UPDATE wallet_challenges SET message=?,expires_at=? WHERE agent_id=?",
                rusqlite::params![expired_message, expires, agent.id],
            )
            .unwrap();
        expired_message.as_str()
    } else {
        original
    };
    let digest = Keccak256::new()
        .chain_update(format!("\x19Ethereum Signed Message:\n{}", message.len()))
        .chain_update(message.as_bytes());
    let (signature, recovery) = key.sign_digest_recoverable(digest).unwrap();
    let mut bytes = signature.to_bytes().to_vec();
    bytes.push(recovery.to_byte() + 27);
    let signed = format!("0x{}", hex::encode(bytes));
    assert_eq!(bnb::personal_signer(message, &signed).unwrap(), wallet);
    let instruction = f.state.registration_instruction(&agent, &wallet).unwrap();
    let mut tx = f.replies.lock().unwrap()["eth_getTransactionByHash"].clone();
    tx["from"] = json!(wallet);
    tx["input"] = json!(instruction.data);
    f.replies
        .lock()
        .unwrap()
        .insert("eth_getTransactionByHash".into(), tx);
    let agent_state = encoded(&[Token::Tuple(vec![
        bnb::addr(&wallet).unwrap(),
        Token::String(agent.plan.name.clone()),
        uint(units(agent.plan.daily_cap).unwrap()),
        bnb::bytes32(&f.state.policy(&agent)).unwrap(),
        Token::Bool(false),
        uint(1),
        uint(0),
        uint(0),
    ])]);
    let function = abi().function("getAgent").unwrap().clone();
    f.replies.lock().unwrap().insert(
        format!("call:0x{}", hex::encode(function.short_signature())),
        agent_state,
    );
    let proof = WalletProof {
        wallet: wallet.clone(),
        signature: signed,
        tx_hash: TXHASH.into(),
    };
    let mut bad = proof.clone();
    bad.wallet = WALLET.into();
    assert!(f.state.register("owner", &agent.id, bad).await.is_err());
    let correct_tx = f.replies.lock().unwrap()["eth_getTransactionByHash"].clone();
    f.replies
        .lock()
        .unwrap()
        .get_mut("eth_getTransactionByHash")
        .unwrap()["input"] = json!("0xffff");
    assert!(f
        .state
        .register("owner", &agent.id, proof.clone())
        .await
        .is_err());
    f.replies
        .lock()
        .unwrap()
        .insert("eth_getTransactionByHash".into(), correct_tx);
    let registered = f
        .state
        .register("owner", &agent.id, proof.clone())
        .await
        .unwrap();
    assert_eq!(registered.registry_id, Some(address));
    assert_eq!(registered.wallet, Some(wallet));
    assert_eq!(registered.status, "ready");
    assert!(f.state.register("owner", &agent.id, proof).await.is_ok());
}
#[tokio::test]
async fn registration_requires_owned_namespace_signed_challenge_and_exact_policy() {
    registration_case(false).await;
}
#[tokio::test]
async fn expired_registration_challenge_can_reconcile_only_its_exact_finalized_transaction() {
    registration_case(true).await;
}
#[tokio::test]
async fn pending_hash_survives_expiry_and_only_final_reverts_release_it() {
    let f = fixture().await;
    let agent=f.state.create_agent("owner",serde_json::from_value(json!({"name":"willow","purpose":"check primary sources","daily_cap":"10","max_call":"0.1","tools":["bnb-rpc"]})).unwrap()).unwrap();
    let intent = TransactionIntent {
        tx_hash: None,
        id: "intent".into(),
        agent_id: agent.id.clone(),
        action: "fund_spending".into(),
        chain_id: 56,
        network: "mainnet".into(),
        sender: WALLET.into(),
        to: PROTOCOL.into(),
        data: "0x1234".into(),
        value: "0x0".into(),
        transaction: ix().transaction(),
        transactions: vec![ix().transaction()],
        expires_at: (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339(),
        details: json!({"amount":"1"}),
    };
    f.state
        .store
        .connect()
        .unwrap()
        .execute(
            "INSERT INTO wallet_intents(id,owner,agent_id,payload,instruction) VALUES(?,?,?,?,?)",
            rusqlite::params![
                intent.id,
                "owner",
                agent.id,
                crate::db::encode(&intent).unwrap(),
                crate::db::encode(&ix()).unwrap()
            ],
        )
        .unwrap();
    assert!(f
        .state
        .wallet_actions("owner", &agent.id)
        .unwrap()
        .is_empty());
    assert!(f
        .state
        .submitted_action("other", "intent", TXHASH, false)
        .await
        .is_err());
    f.state
        .submitted_action("owner", "intent", TXHASH, false)
        .await
        .unwrap();
    let pending = f.state.wallet_actions("owner", &agent.id).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].tx_hash.as_deref(), Some(TXHASH));
    assert!(f
        .state
        .release_failed_action("owner", "intent", false)
        .await
        .is_err());
    let mut receipt = f.replies.lock().unwrap()["eth_getTransactionReceipt"].clone();
    receipt["status"] = json!("0x0");
    f.replies
        .lock()
        .unwrap()
        .insert("eth_getTransactionReceipt".into(), receipt);
    f.replies
        .lock()
        .unwrap()
        .insert("eth_blockNumber".into(), json!("0x64"));
    assert!(f
        .state
        .release_failed_action("owner", "intent", false)
        .await
        .is_err());
    f.replies
        .lock()
        .unwrap()
        .insert("eth_blockNumber".into(), json!("0x66"));
    f.state
        .release_failed_action("owner", "intent", false)
        .await
        .unwrap();
    assert!(f
        .state
        .wallet_actions("owner", &agent.id)
        .unwrap()
        .is_empty());
}
#[tokio::test]
async fn public_payment_totals_require_a_verified_receipt_and_count_each_hash_once() {
    let f = fixture().await;
    let agent=f.state.create_agent("owner",serde_json::from_value(json!({"name":"willow","purpose":"check primary sources","daily_cap":"10","max_call":"0.1","tools":["bnb-rpc"],"public_activity":true})).unwrap()).unwrap();
    for currency in ["USDT", "USDT", "USDC"] {
        f.state
            .event(
                &agent,
                "payment",
                "paid",
                "confirmed",
                Some("merchant"),
                Some("0.000000000000000001"),
                Some(currency),
                Some(TXHASH),
            )
            .unwrap();
    }
    f.state
        .event(
            &agent,
            "payment",
            "unverified",
            "confirmed",
            Some("merchant"),
            Some("9"),
            Some("USDT"),
            Some(BLOCK),
        )
        .unwrap();
    assert_eq!(f.state.public_records().unwrap()[0].payments, 0);
    f.state.store.claim_receipt(TXHASH, "payment:one").unwrap();
    let records = f.state.public_records().unwrap();
    assert_eq!(records[0].payments, 1);
    assert_eq!(records[0].paid.to_string(), "0.000000000000000001");
}
#[tokio::test]
async fn invoice_matches_independent_viem_eip712_vector_and_rejects_term_changes() {
    let f = fixture().await;
    let input = || {
        serde_json::from_value(json!({"name":"willow","purpose":"check primary sources","daily_cap":"100","max_call":"10","tools":["x402"]})).unwrap()
    };
    let buyer = f.state.create_agent("owner", input()).unwrap();
    let executor = f.state.create_agent("owner", input()).unwrap();
    let plan=serde_json::from_value(json!({"title":"review source documents","description":"compare primary sources and provide links","executor_id":executor.id,"budget":"50","max_call":"10","deadline":(chrono::Utc::now()+chrono::Duration::hours(4)).to_rfc3339(),"tools":["x402"]})).unwrap();
    let mut job = f
        .state
        .create_job("owner", &buyer.id, plan, None)
        .await
        .unwrap();
    job.id = "ab".repeat(32);
    job.terms_hash = format!("0x{}", "cd".repeat(32));
    let merchant = JobMerchant {
        id: "reports".into(),
        name: "reports".into(),
        tool: "x402".into(),
        service_key: "0x19e7e376e7c213b7e7e7e46cc70a5dd086daff2a".into(),
        recipient: BACKING.into(),
    };
    let invoice=ServiceInvoice{service_id:"reports".into(),request_hash:format!("0x{}","ef".repeat(32)),amount:rust_decimal::Decimal::from_str("30.000000000000000001").unwrap(),expires:2000000000,nonce:format!("0x{}","12".repeat(32)),signature:"0xb387875b9de67860c936b436f2b2ab19e7f1868d7b72384e34ff56968a588d76261cb68d94d8bd10db9e7f1f9b5e827530a29e997fa95ebab92c6a88f21ca12d1b".into()};
    let digest = crate::invoices::invoice_digest(WALLET, &job, &merchant, &invoice).unwrap();
    assert_eq!(
        hex::encode(&digest),
        "d21a552d2c70f8d39b0109c6597023652b40d4f2fa9cc3ef465f5b1ebe8be7e6"
    );
    assert_eq!(
        bnb::digest_signer(&digest, &invoice.signature).unwrap(),
        merchant.service_key
    );
    for field in ["amount", "expires", "nonce", "request_hash"] {
        let mut changed = invoice.clone();
        match field {
            "amount" => changed.amount += rust_decimal::Decimal::ONE,
            "expires" => changed.expires += 1,
            "nonce" => changed.nonce = format!("0x{}", "34".repeat(32)),
            _ => changed.request_hash = format!("0x{}", "56".repeat(32)),
        };
        let digest = crate::invoices::invoice_digest(WALLET, &job, &merchant, &changed).unwrap();
        assert_ne!(
            bnb::digest_signer(&digest, &invoice.signature).unwrap(),
            merchant.service_key
        );
    }
    let mut changed = job.clone();
    changed.terms_hash = format!("0x{}", "ab".repeat(32));
    assert_ne!(
        crate::invoices::invoice_digest(WALLET, &changed, &merchant, &invoice).unwrap(),
        digest
    );
    changed = job.clone();
    changed.id = "cd".repeat(32);
    assert_ne!(
        crate::invoices::invoice_digest(WALLET, &changed, &merchant, &invoice).unwrap(),
        digest
    );
    let mut changed = merchant.clone();
    changed.id = "other".into();
    assert_ne!(
        crate::invoices::invoice_digest(WALLET, &job, &changed, &invoice).unwrap(),
        digest
    );
    changed = merchant.clone();
    changed.recipient = ECONOMICS.into();
    assert_ne!(
        crate::invoices::invoice_digest(WALLET, &job, &changed, &invoice).unwrap(),
        digest
    );
    assert_ne!(
        crate::invoices::invoice_digest(PROTOCOL, &job, &merchant, &invoice).unwrap(),
        digest
    );
}

#[tokio::test]
async fn token_metadata_checks_chain_code_and_exact_supported_precision() {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let f = fixture().await;
    let path = format!("/api/bnb/tokens/{WALLET}");
    for decimals in [0, 6, 8, 18] {
        f.replies.lock().unwrap().insert(
            format!("call:{}", selector("decimals()")),
            encoded(&[uint(decimals)]),
        );
        let response = crate::api::router(f.state.clone())
            .oneshot(Request::builder().uri(&path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(response.status().is_success());
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["chain_id"], 56);
        assert_eq!(body["decimals"], json!(decimals));
        assert_eq!(body["address"], WALLET);
    }
    for (key, value) in [
        (
            format!("call:{}", selector("decimals()")),
            encoded(&[uint(19)]),
        ),
        ("eth_getCode".into(), json!("0x")),
        ("eth_chainId".into(), json!("0x1")),
    ] {
        f.replies.lock().unwrap().insert(key.clone(), value);
        let response = crate::api::router(f.state.clone())
            .oneshot(Request::builder().uri(&path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(!response.status().is_success(), "{key}");
        f.replies.lock().unwrap().insert(
            format!("call:{}", selector("decimals()")),
            encoded(&[uint(18)]),
        );
        f.replies
            .lock()
            .unwrap()
            .insert("eth_getCode".into(), json!("0x60006000"));
    }
}

#[tokio::test]
async fn public_transaction_status_binds_recovery_envelope_to_canonical_failed_receipt() {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let f = fixture().await;
    f.replies
        .lock()
        .unwrap()
        .get_mut("eth_getTransactionReceipt")
        .unwrap()["status"] = json!("0x0");
    let path = format!("/api/bnb/transactions/{TXHASH}");
    let response = crate::api::router(f.state.clone())
        .oneshot(Request::builder().uri(&path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert!(response.status().is_success());
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["status"], "failed");
    assert_eq!(body["transaction"]["from"], WALLET);
    assert_eq!(body["transaction"]["input"], "0x1234");
    f.replies
        .lock()
        .unwrap()
        .get_mut("eth_getTransactionByHash")
        .unwrap()["blockHash"] = json!(bnb::ZERO_HASH);
    let response = crate::api::router(f.state.clone())
        .oneshot(Request::builder().uri(&path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert!(!response.status().is_success());
}

#[tokio::test]
async fn cached_registry_endpoint_does_not_wait_for_slow_deployment_rpc_or_publish_partial_records() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let f = fixture().await;
    f.replies
        .lock()
        .unwrap()
        .insert("delay:eth_chainId".into(), json!(20_000));
    let started = std::time::Instant::now();
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(14),
        crate::api::router(f.state.clone()).oneshot(
            Request::builder()
                .uri("/api/registry")
                .body(Body::empty())
                .unwrap(),
        ),
    )
    .await
    .expect("Registry must finish before the proxy timeout")
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(started.elapsed() < std::time::Duration::from_millis(250));
    let payload: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(payload["status"], "checking");
    assert_eq!(payload["agents"], json!([]));
    assert_eq!(payload["transactions"], json!([]));
    for key in [
        "address",
        "owner",
        "block_number",
        "verified_at",
        "fees_bnb",
    ] {
        assert!(payload[key].is_null(), "No unverified {key} on timeout");
    }
    assert!(payload["error"].is_null());
    assert_eq!(payload["discovery"]["status"], "checking");
    assert!(f.replies.lock().unwrap().get("request:eth_chainId").is_none());
    let error=tokio::time::timeout(std::time::Duration::from_secs(13),f.state.refresh_registry()).await.unwrap().unwrap_err();
    assert!(error.1.contains("timed out"));
    let registry=f.state.registry().await;
    assert!(registry.agents.is_empty());
    assert!(registry.owner.is_none());
    assert!(registry.verified_at.is_none());
}

#[tokio::test]
async fn legacy_secured_assets_are_unsupported_and_stock_module_is_separate() {
    let f = fixture().await;
    let assets = f.state.secured_assets().await.unwrap();
    assert_eq!(assets["status"], "unsupported_legacy");
    assert_eq!(assets["supported"], false);
    assert_eq!(assets["stock_loans"]["status"], "not_deployed");
    for asset in assets["assets"].as_array().unwrap() {
        assert_eq!(asset["borrowing_enabled"], false);
        assert_eq!(asset["borrowing_status"], "unsupported_legacy");
        assert!(asset["risk"].is_null());
        assert!(asset["price_usdt"].is_null());
    }
    assert!(f.replies.lock().unwrap().keys().all(|key| !key.starts_with("request:")));
}

#[tokio::test]
async fn secured_open_credit_uses_collateral_terms_and_exact_usdt_approval() {
    const SIGNER: &str = "0x7777777777777777777777777777777777777777";
    const MERCHANT: &str = "0x9999999999999999999999999999999999999999";
    let f = fixture().await;
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&f.state.config.manifest).unwrap()).unwrap();
    manifest["credit_mode"] = json!("collateralized");
    std::fs::write(&f.state.config.manifest, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let mut agents = vec![];
    for name in ["lender", "borrower"] {
        let mut agent = f.state.create_agent("owner", serde_json::from_value(json!({"name":name,"purpose":"monitor a public wallet","daily_cap":"10","max_call":"1","tools":["bnb-rpc"],"public_activity":false})).unwrap()).unwrap();
        agent.wallet = Some(WALLET.into());
        agent.registry_id = Some(f.state.bnb.agent_address(WALLET, &agent.id).unwrap());
        agent.status = "ready".into();
        f.state.store.save_agent(&agent).unwrap();
        agents.push(agent);
    }
    let lender = &agents[0];
    let borrower = &agents[1];
    let expires = chrono::Utc::now().timestamp() + 3600;
    let principal = 3_123_456_789_012_345_678;
    std::fs::write(&f.state.config.x402_merchants, serde_json::to_vec(&json!([{"id":"credit-fixture","resource_url":"https://merchant.example/data","facilitator_url":"https://facilitator.example","recipient":MERCHANT,"network":"eip155:56","asset":bnb::USDT,"max_amount":"1000000000000"}])).unwrap()).unwrap();
    {
        let mut replies = f.replies.lock().unwrap();
        replies.insert(format!("call:{}",selector("collateralAssets(address)")),encoded(&[bnb::addr(bnb::ZERO).unwrap(),uint(9000),uint(9500),uint(200),uint(18),Token::Bool(false),uint(1_000_000_000_000_000_000_000_000),uint(0)]));
        replies.insert(
            format!("call:{}", selector("getAgent(bytes32)")),
            encoded(&[Token::Tuple(vec![
                bnb::addr(WALLET).unwrap(),
                Token::String("agent".into()),
                uint(10_000_000_000_000_000_000),
                Token::FixedBytes(vec![1; 32]),
                Token::Bool(false),
                uint(1),
                uint(0),
                uint(0),
            ])]),
        );
        replies.insert(
            format!("call:{}", selector("allowance(address,address)")),
            encoded(&[uint(1)]),
        );
    }
    let input = serde_json::from_value(json!({"action":"open_credit","amount":"3.123456789012345678","credit_id":TXHASH,"target_agent_id":borrower.id,"session_signer":SIGNER,"per_call":"0.25","daily_cap":"2","session_expires_at":expires,"tools":["bnb-rpc"],"recipients":[MERCHANT]})).unwrap();
    let intent = f
        .state
        .financial_prepare("owner", &lender.id, input)
        .await
        .unwrap();
    assert_eq!(intent.action, "open_credit");
    assert_eq!(intent.sender, WALLET);
    assert_eq!(intent.to, BACKING);
    assert_eq!(intent.chain_id, 56);
    assert_eq!(intent.value, "0x0");
    assert_eq!(
        intent.transactions.len(),
        3,
        "Existing allowance is reset before exact approval"
    );
    for (transaction, amount) in intent.transactions[..2].iter().zip([0, principal]) {
        assert_eq!(transaction.to, bnb::USDT.to_lowercase());
        assert_eq!(transaction.chain_id, "0x38");
        assert_eq!(transaction.value, "0x0");
        let calldata = hex::decode(transaction.data.trim_start_matches("0x")).unwrap();
        assert_eq!(
            format!("0x{}", hex::encode(&calldata[..4])),
            selector("approve(address,uint256)")
        );
        let args = ethabi::decode(
            &[ethabi::ParamType::Address, ethabi::ParamType::Uint(256)],
            &calldata[4..],
        )
        .unwrap();
        assert_eq!(args, vec![bnb::addr(BACKING).unwrap(), uint(amount)]);
    }
    assert_eq!(intent.transactions[2].to, BACKING);
    assert_eq!(intent.transactions[2].data, intent.data);
    let contract = ethabi::Contract::load(
        include_bytes!("../../contracts/bnb/abi/TabBacking.json").as_slice(),
    )
    .unwrap();
    let method = contract.function("openCreditWithCollateral").unwrap();
    let calldata = hex::decode(intent.data.trim_start_matches("0x")).unwrap();
    assert_eq!(&calldata[..4], &method.short_signature());
    assert_eq!(
        method.decode_input(&calldata[4..]).unwrap(),
        vec![Token::Tuple(vec![
            bnb::bytes32(TXHASH).unwrap(),
            bnb::bytes32(borrower.registry_id.as_deref().unwrap()).unwrap(),
            bnb::addr(SIGNER).unwrap(),
            uint(principal),
            uint(250_000_000_000_000_000),
            uint(2_000_000_000_000_000_000),
            uint(expires as u128),
            uint(1),
            Token::Array(vec![bnb::addr(MERCHANT).unwrap()])
        ]),bnb::addr(bnb::USDT).unwrap()]
    );
    assert_eq!(
        f.state.wallet_actions("owner", &lender.id).unwrap().len(),
        1
    );
    let replies = f.replies.lock().unwrap();
    for signature in [
        "collateralPositions(bytes32)",
        "collateralPrice(address)",
    ] {
        assert!(!replies.contains_key(&format!("request:call:{}", selector(signature))));
    }
}

#[tokio::test]
async fn legacy_collateral_preparation_fails_before_rpc_or_wallet_intent() {
    let f = fixture().await;
    for action in [
        "pledge_collateral",
        "withdraw_collateral",
        "liquidate_credit",
    ] {
        let input =
            serde_json::from_value(json!({"action":action,"amount":"1","credit_id":TXHASH}))
                .unwrap();
        let error = f
            .state
            .financial_prepare("owner", "unregistered", input)
            .await
            .err()
            .unwrap();
        assert_eq!(error.0, axum::http::StatusCode::SERVICE_UNAVAILABLE);
        assert!(error.1.contains("collateralized deployment"));
    }
    let intents: u64 = f
        .state
        .store
        .connect()
        .unwrap()
        .query_row("SELECT count(*) FROM wallet_intents", [], |row| row.get(0))
        .unwrap();
    assert_eq!(intents, 0);
    assert!(f
        .replies
        .lock()
        .unwrap()
        .keys()
        .all(|key| !key.starts_with("request:")));
}

#[tokio::test]
async fn token_fee_reports_verified_legacy_rate_until_new_contract_is_active() {
    let f=fixture().await;
    let system=f.state.token_system().await;
    assert_eq!(system["fee_bps"],200);
    assert_eq!(system["holder_exemption_enabled"],false);
    assert_eq!(system["official_tab_address"],Value::Null);
}
