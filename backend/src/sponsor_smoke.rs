//! Explicitly authorized, ignored mainnet smoke. Never included in the release binary.
//! A local RPC gate permits one exact registration and caps its worst-case gas cost.
use crate::{bnb, config::Config, models::*, AppState};
use axum::{extract::State, routing::post, Json, Router};
use ethabi::Token;
use k256::ecdsa::SigningKey;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

const OWNER: &str = "0xcaaf7c62ca25081e483bf60eea68f779f218fdc2";
const SPONSOR: &str = "0xa99cf06fcde993a6d2faa73a2c82d67980fd0416";
const PROTOCOL: &str = "0x567e7187d477b1a68c3ac3d292ad44a0d5e770c7";
const ACCOUNT: &str = "did:privy:tab-registration-smoke";
const NAME: &str = "tab registration check";
const MAX_WEI: u64 = 50_000_000_000_000;
const AUTHORIZATION: &str = "chain56-register-only-max-0.00005-bnb";
const RPC: &str = "https://bsc-dataseed.bnbchain.org";

#[derive(Default)]
struct Gate {
    calldata: Option<String>,
    transaction_hash: Option<String>,
    refusal: bool,
}
type GateState = (reqwest::Client, Arc<Mutex<Gate>>);

fn signer_address(key: &SigningKey) -> String {
    let point = key.verifying_key().to_encoded_point(false);
    format!(
        "0x{}",
        hex::encode(&Keccak256::digest(&point.as_bytes()[1..])[12..])
    )
}

fn bounded_transaction(raw: &str, expected_data: &str) -> Option<String> {
    let bytes = hex::decode(raw.strip_prefix("0x")?).ok()?;
    let tx = rlp::Rlp::new(&bytes);
    if tx.item_count().ok()? != 9 {
        return None;
    }
    let gas_price: u64 = tx.val_at(1).ok()?;
    let gas: u64 = tx.val_at(2).ok()?;
    let recipient: Vec<u8> = tx.val_at(3).ok()?;
    let value: u64 = tx.val_at(4).ok()?;
    let data: Vec<u8> = tx.val_at(5).ok()?;
    let v: u64 = tx.val_at(6).ok()?;
    if gas == 0
        || gas > 500_000
        || gas_price == 0
        || gas_price > 1_000_000_000
        || gas.checked_mul(gas_price)? > MAX_WEI
        || value != 0
        || format!("0x{}", hex::encode(&recipient)) != PROTOCOL
        || format!("0x{}", hex::encode(&data)) != expected_data
        || ![147, 148].contains(&v)
    {
        return None;
    }
    let abi = ethabi::Contract::load(std::io::Cursor::new(include_str!(
        "../../contracts/bnb/abi/TabProtocol.json"
    )))
    .ok()?;
    let register = abi.function("registerWithSignature").ok()?;
    if data.len() < 4 || data[..4] != register.short_signature() {
        return None;
    }
    let fields = register.decode_input(&data[4..]).ok()?;
    let id = fields.first()?.clone().into_fixed_bytes()?;
    if id.len() != 32
        || format!("0x{}", hex::encode(&id[..20])) != OWNER
        || fields.get(1)? != &bnb::addr(OWNER).ok()?
        || fields.get(2)? != &Token::String(NAME.into())
        || fields.get(3)? != &bnb::uint(1_000_000_000_000_000_000)
    {
        return None;
    }
    let mut unsigned = rlp::RlpStream::new_list(9);
    for index in 0..6 {
        unsigned.append_raw(tx.at(index).ok()?.as_raw(), 1);
    }
    unsigned.append(&56u64).append(&0u8).append(&0u8);
    let r: Vec<u8> = tx.val_at(7).ok()?;
    let s: Vec<u8> = tx.val_at(8).ok()?;
    if r.len() > 32 || s.len() > 32 {
        return None;
    }
    let mut signature = [0u8; 65];
    signature[32 - r.len()..32].copy_from_slice(&r);
    signature[64 - s.len()..64].copy_from_slice(&s);
    signature[64] = (v - 147 + 27) as u8;
    if bnb::digest_signer(
        &Keccak256::digest(unsigned.out()),
        &format!("0x{}", hex::encode(signature)),
    )
    .ok()?
    .as_str()
        != SPONSOR
    {
        return None;
    }
    Some(bnb::hash(&bytes))
}

async fn guarded_rpc(
    State((client, gate)): State<GateState>,
    Json(request): Json<Value>,
) -> Json<Value> {
    let denied = || {
        Json(
            json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":"Smoke RPC request refused"}}),
        )
    };
    let Some(method) = request["method"].as_str() else {
        return denied();
    };
    if method == "eth_sendRawTransaction" {
        let allowed = {
            let mut gate = gate.lock().unwrap();
            let candidate = request["params"][0]
                .as_str()
                .zip(gate.calldata.as_deref())
                .and_then(|(raw, data)| bounded_transaction(raw, data));
            match candidate {
                Some(hash)
                    if gate
                        .transaction_hash
                        .as_ref()
                        .is_none_or(|saved| *saved == hash) =>
                {
                    gate.transaction_hash = Some(hash);
                    true
                }
                _ => {
                    gate.refusal = true;
                    false
                }
            }
        };
        if !allowed {
            return denied();
        }
    } else if ![
        "eth_chainId",
        "eth_blockNumber",
        "eth_getBlockByNumber",
        "eth_getBalance",
        "eth_getCode",
        "eth_call",
        "eth_getTransactionByHash",
        "eth_getTransactionReceipt",
        "eth_estimateGas",
        "eth_getStorageAt",
        "eth_gasPrice",
        "eth_getTransactionCount",
    ]
    .contains(&method)
    {
        return denied();
    }
    let Ok(response) = client.post(RPC).json(&request).send().await else {
        return denied();
    };
    let Ok(response) = response.error_for_status() else {
        return denied();
    };
    match response.json::<Value>().await {
        Ok(value) => Json(value),
        Err(_) => denied(),
    }
}

fn signature(challenge: &SponsoredChallenge, key: &SigningKey) -> String {
    let value = &challenge.typed_data;
    let m = &value["message"];
    let h = |text: &str| Token::FixedBytes(Keccak256::digest(text.as_bytes()).to_vec());
    let domain = Keccak256::digest(ethabi::encode(&[
        h("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"),
        h("Tab Protocol"),
        h("1"),
        bnb::uint(56),
        bnb::addr(PROTOCOL).unwrap(),
    ]));
    let uint = |name: &str| bnb::uint(m[name].as_str().unwrap().parse().unwrap());
    let payload = Keccak256::digest(ethabi::encode(&[
        h("Register(bytes32 id,address owner,string name,uint256 dailyCap,bytes32 policyHash,uint256 nonce,uint256 deadline)"),
        bnb::bytes32(m["id"].as_str().unwrap()).unwrap(), bnb::addr(OWNER).unwrap(), h(NAME),
        uint("dailyCap"), bnb::bytes32(m["policyHash"].as_str().unwrap()).unwrap(), uint("nonce"), uint("deadline"),
    ]));
    let digest = Keccak256::new()
        .chain_update([0x19, 0x01])
        .chain_update(domain)
        .chain_update(payload)
        .finalize();
    let (signed, recovery) = key
        .sign_prehash_recoverable(&digest)
        .expect("Smoke signature failed");
    let mut bytes = signed.to_bytes().to_vec();
    bytes.push(recovery.to_byte() + 27);
    format!("0x{}", hex::encode(bytes))
}

#[tokio::test]
#[ignore = "Spends at most 0.00005 BNB once; requires explicit authorization and two Vault aliases"]
async fn mainnet_zero_bnb_owner_registration() {
    assert!(
        std::env::var("TAB_SPONSOR_SMOKE_AUTHORIZED")
            .ok()
            .as_deref()
            == Some(AUTHORIZATION),
        "Mainnet smoke is not authorized"
    );
    let secret = |name| std::env::var(name).expect("Inject the designated Vault aliases");
    let owner_secret = secret("TAB_SPONSOR_SMOKE_OWNER_KEY");
    let sponsor_secret = secret("TAB_BNB_SPONSOR_KEY");
    let key = |value: &str| {
        SigningKey::from_slice(
            &hex::decode(value.trim_start_matches("0x")).expect("Invalid smoke key encoding"),
        )
        .expect("Invalid smoke key")
    };
    let owner_key = key(&owner_secret);
    assert!(
        signer_address(&owner_key) == OWNER,
        "Wrong smoke owner alias"
    );
    assert!(
        signer_address(&key(&sponsor_secret)) == SPONSOR,
        "Wrong sponsor alias"
    );
    drop(owner_secret);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let directory = PathBuf::from("/tmp/tab-bnb-registration-smoke-20261008");
    if directory.exists() {
        assert!(!directory
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink());
    }
    std::fs::create_dir_all(&directory).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let gate = Arc::new(Mutex::new(Gate::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let rpc = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/", post(guarded_rpc))
        .with_state((client, gate.clone()));
    let proxy = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let config = Config {
        sponsor_enabled: true,
        sponsor_key: Some(sponsor_secret),
        sponsor_address: Some(SPONSOR.into()),
        network: "mainnet".into(),
        chain_id: 56,
        confirmations: 3,
        rpc,
        logs_rpc: None,
        usdt: bnb::USDT.into(),
        program: PROTOCOL.into(),
        official_tab: None,
        app_id: None,
        database: directory.join("smoke.sqlite"),
        manifest: root.join("contracts/deployments/bnb-56.json"),
        merchants: directory.join("unused-merchants.json"),
        x402_merchants: directory.join("unused-x402.json"),
        bind: "127.0.0.1:0".into(),
        openrouter_key: None,
        tavily_key: None,
        inference_daily_micros: 0,
    };
    let state = AppState::new(config).unwrap();
    state.bnb.require_deployment().await.unwrap();
    assert_eq!(
        bnb::number(
            &state
                .bnb
                .rpc("eth_getBalance", json!([OWNER, "latest"]))
                .await
                .unwrap()
        )
        .unwrap(),
        0
    );
    let agents = state.store.list_agents(ACCOUNT).unwrap();
    assert!(agents.len() <= 1, "Smoke database has unexpected agents");
    let agent = if let Some(agent) = agents.into_iter().next() {
        agent
    } else {
        let input: AgentInput = serde_json::from_value(json!({"name":NAME,"purpose":"team registration gas sponsorship check","tools":["bnb-rpc"],"daily_cap":"1","max_call":"0.1","public_activity":true})).unwrap();
        state.create_agent(ACCOUNT, input).unwrap()
    };
    assert_eq!(agent.plan.name, NAME);
    let existing: Option<String> = {
        use rusqlite::OptionalExtension;
        state.store.connect().unwrap().query_row("SELECT payload FROM sponsor_jobs WHERE owner=? AND agent_id=? ORDER BY rowid DESC LIMIT 1",[ACCOUNT,&agent.id],|row|row.get(0)).optional().unwrap()
    };
    let mut result = if let Some(payload) = existing {
        let saved: Value = serde_json::from_str(&payload).unwrap();
        let mut locked = gate.lock().unwrap();
        locked.calldata = Some(saved["instruction"]["data"].as_str().unwrap().into());
        locked.transaction_hash = Some(saved["tx_hash"].as_str().unwrap().into());
        drop(locked);
        state.sponsor_status(ACCOUNT, &agent.id).await.unwrap()
    } else {
        let challenge = state
            .sponsored_challenge(ACCOUNT, &agent.id, OWNER)
            .await
            .unwrap();
        assert_eq!(challenge.chain_id, 56);
        assert_eq!(challenge.registry, PROTOCOL);
        assert_eq!(challenge.wallet, OWNER);
        assert_eq!(challenge.typed_data["message"]["name"], NAME);
        assert_eq!(
            challenge.typed_data["message"]["dailyCap"],
            "1000000000000000000"
        );
        let signed = signature(&challenge, &owner_key);
        let m = &challenge.typed_data["message"];
        let ix = state
            .bnb
            .instruction(
                "protocol",
                "registerWithSignature",
                vec![
                    bnb::bytes32(&challenge.registry_id).unwrap(),
                    bnb::addr(OWNER).unwrap(),
                    Token::String(NAME.into()),
                    bnb::uint(1_000_000_000_000_000_000),
                    bnb::bytes32(&challenge.policy_hash).unwrap(),
                    bnb::uint(m["nonce"].as_str().unwrap().parse().unwrap()),
                    bnb::uint(m["deadline"].as_str().unwrap().parse().unwrap()),
                    Token::Bytes(hex::decode(&signed[2..]).unwrap()),
                ],
            )
            .unwrap();
        gate.lock().unwrap().calldata = Some(ix.data);
        state
            .sponsor_registration(
                ACCOUNT,
                &agent.id,
                RegistrationSignature {
                    wallet: OWNER.into(),
                    signature: signed,
                    request_id: challenge.request_id,
                },
            )
            .await
            .unwrap()
    };
    for _ in 0..60 {
        assert!(!gate.lock().unwrap().refusal,"Smoke transaction exceeded its exact scope or gas ceiling; nothing outside that scope was sent");
        if result.status != "pending" {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        result = state.sponsor_status(ACCOUNT, &agent.id).await.unwrap();
    }
    assert_eq!(
        result.status, "confirmed",
        "Smoke remains retained in its private database; reconcile before retrying"
    );
    let registered = result.agent.unwrap();
    assert_eq!(registered.wallet.as_deref(), Some(OWNER));
    let onchain = state
        .bnb
        .agent(registered.registry_id.as_ref().unwrap())
        .await
        .unwrap();
    assert_eq!(onchain["owner"], OWNER);
    assert_eq!(onchain["name"], NAME);
    assert_eq!(
        bnb::number(
            &state
                .bnb
                .rpc("eth_getBalance", json!([OWNER, "latest"]))
                .await
                .unwrap()
        )
        .unwrap(),
        0
    );
    let tx_hash = result.tx_hash.unwrap();
    let status = state.bnb.transaction_status(&tx_hash).await.unwrap();
    assert_eq!(status["status"], "confirmed");
    let gas_cost = bnb::number(&status["receipt"]["gasUsed"]).unwrap()
        * bnb::number(&status["receipt"]["effectiveGasPrice"]).unwrap();
    assert!(gas_cost <= u128::from(MAX_WEI));
    println!(
        "{}",
        json!({"status":"confirmed","proof":"real backend sponsored registration methods","chain_id":56,"sponsor":SPONSOR,"owner":OWNER,"owner_balance_wei":"0","agent_id":registered.registry_id,"transaction_hash":tx_hash,"gas_cost_wei":gas_cost.to_string(),"registration_only":true})
    );
    proxy.abort();
}
