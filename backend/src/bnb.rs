//! BNB mainnet read transport, ABI encoding and wallet transaction proof checks.
//! Signing is isolated in the bounded registration sponsor module.
use crate::{
    config::Config,
    error::{ApiError, Result},
    models::EvmTransaction,
};
use ethabi::{
    ethereum_types::{H160, U256},
    Contract, Token,
};
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::{
    io::Cursor,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
pub const CHAIN_ID: u64 = 56;
pub const USDT: &str = "0x55d398326f99059ff775485246999027b3197955";
pub const ZERO: &str = "0x0000000000000000000000000000000000000000";
pub const ZERO_HASH: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";
pub fn address(value: &str) -> Result<String> {
    if value.len() != 42 || !value.starts_with("0x") {
        return Err(ApiError::validation(
            "Use an EVM address with 40 hexadecimal digits.",
        ));
    }
    H160::from_str(value)
        .map(|v| format!("{v:#x}"))
        .map_err(|_| ApiError::validation("Use a valid EVM address."))
}
pub fn pubkey(value: &str) -> Result<String> {
    address(value)
}
pub fn signature(value: &str) -> Result<Vec<u8>> {
    if value.len() != 66 || !value.starts_with("0x") {
        return Err(ApiError::validation("Use a 32-byte transaction hash."));
    }
    hex::decode(&value[2..]).map_err(|_| ApiError::validation("Invalid transaction hash."))
}
pub fn hash(value: &[u8]) -> String {
    format!("0x{}", hex::encode(Keccak256::digest(value)))
}
pub fn id(value: &str) -> Result<String> {
    let raw = value.strip_prefix("0x").unwrap_or(value);
    if ![32, 64].contains(&raw.len()) || hex::decode(raw).is_err() {
        return Err(ApiError::validation(
            "Invalid agent or commitment identifier.",
        ));
    }
    Ok(format!("0x{raw:0>64}"))
}
pub fn addr(value: &str) -> Result<Token> {
    Ok(Token::Address(
        H160::from_str(&address(value)?).map_err(|_| ApiError::internal())?,
    ))
}
pub fn bytes32(value: &str) -> Result<Token> {
    Ok(Token::FixedBytes(
        hex::decode(id(value)?.trim_start_matches("0x")).map_err(|_| ApiError::internal())?,
    ))
}
pub fn uint(value: u128) -> Token {
    Token::Uint(U256::from(value))
}
pub fn number(value: &Value) -> Result<u128> {
    let s = value
        .as_str()
        .ok_or_else(|| ApiError::unavailable("Invalid contract integer."))?;
    let v = if let Some(s) = s.strip_prefix("0x") {
        u128::from_str_radix(s, 16)
    } else {
        s.parse()
    }
    .map_err(|_| ApiError::unavailable("Contract amount exceeds supported limits."))?;
    if v > 79_228_162_514_264_337_593_543_950_335u128 {
        return Err(ApiError::unavailable(
            "Contract amount exceeds exact decimal limits.",
        ));
    }
    Ok(v)
}
pub fn digest_signer(digest: &[u8], signature: &str) -> Result<String> {
    let bytes = hex::decode(signature.strip_prefix("0x").unwrap_or(""))
        .map_err(|_| ApiError::forbidden("Invalid wallet signature."))?;
    if bytes.len() != 65 {
        return Err(ApiError::forbidden("Use a 65-byte wallet signature."));
    }
    let sig = Signature::from_slice(&bytes[..64])
        .map_err(|_| ApiError::forbidden("Invalid wallet signature."))?;
    if sig.normalize_s().is_some() {
        return Err(ApiError::forbidden("Noncanonical wallet signature."));
    }
    let v = match bytes[64] {
        27 | 28 => bytes[64] - 27,
        0 | 1 => bytes[64],
        _ => return Err(ApiError::forbidden("Invalid wallet recovery identifier.")),
    };
    let key = VerifyingKey::recover_from_prehash(
        digest,
        &sig,
        RecoveryId::from_byte(v).ok_or_else(|| ApiError::forbidden("Invalid wallet signature."))?,
    )
    .map_err(|_| ApiError::forbidden("Invalid wallet signature."))?;
    let point = key.to_encoded_point(false);
    Ok(format!(
        "0x{}",
        hex::encode(&Keccak256::digest(&point.as_bytes()[1..])[12..])
    ))
}
pub fn personal_signer(message: &str, signature: &str) -> Result<String> {
    let prefix = format!("\x19Ethereum Signed Message:\n{}", message.len());
    digest_signer(
        &Keccak256::new()
            .chain_update(prefix)
            .chain_update(message.as_bytes())
            .finalize(),
        signature,
    )
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Instruction {
    pub to: String,
    pub data: String,
    pub value: String,
}
impl Instruction {
    pub fn transaction(&self) -> EvmTransaction {
        EvmTransaction {
            to: self.to.clone(),
            data: self.data.clone(),
            value: self.value.clone(),
            chain_id: "0x38".into(),
        }
    }
}
#[derive(Clone)]
pub struct Bnb {
    pub config: Arc<Config>,
    pub client: reqwest::Client,
    deployment: Arc<Mutex<Option<(Instant, bool)>>>,
    pub(crate) registry_scan: Arc<Mutex<()>>,
    pub(crate) registry_state: Arc<std::sync::Mutex<crate::registry_cache::RefreshState>>,
}
fn abi_json(module: &str) -> Result<&'static str> {
    match module {
        "protocol" => Ok(include_str!("../../contracts/bnb/abi/TabProtocol.json")),
        "backing" => Ok(include_str!("../../contracts/bnb/abi/TabBacking.json")),
        "economics" => Ok(include_str!("../../contracts/bnb/abi/TabEconomics.json")),
        _ => Err(ApiError::internal()),
    }
}
fn abi(module: &str) -> Result<Contract> {
    Contract::load(Cursor::new(abi_json(module)?.as_bytes())).map_err(|_| ApiError::internal())
}
fn token_json(token: &Token, spec: &Value) -> Value {
    match token {
        Token::Address(a) => json!(format!("{a:#x}")),
        Token::Uint(v) | Token::Int(v) => json!(v.to_string()),
        Token::Bool(v) => json!(v),
        Token::String(v) => json!(v),
        Token::Bytes(v) | Token::FixedBytes(v) => json!(format!("0x{}", hex::encode(v))),
        Token::Tuple(tokens) => {
            let mut o = serde_json::Map::new();
            for (i, t) in tokens.iter().enumerate() {
                let s = &spec["components"][i];
                o.insert(s["name"].as_str().unwrap_or("").into(), token_json(t, s));
            }
            Value::Object(o)
        }
        Token::Array(tokens) | Token::FixedArray(tokens) => json!(tokens
            .iter()
            .map(|t| token_json(t, spec))
            .collect::<Vec<_>>()),
    }
}
impl Bnb {
    pub fn new(config: Arc<Config>, client: reqwest::Client) -> Self {
        Self {
            config,
            client,
            deployment: Arc::new(Mutex::new(None)),
            registry_scan: Arc::new(Mutex::new(())),
            registry_state: Arc::new(std::sync::Mutex::new(Default::default())),
        }
    }
    pub async fn rpc(&self, method: &str, params: Value) -> Result<Value> {
        if ![
            "eth_chainId",
            "eth_blockNumber",
            "eth_getBlockByNumber",
            "eth_getBalance",
            "eth_getCode",
            "eth_call",
            "eth_getTransactionByHash",
            "eth_getTransactionReceipt",
            "eth_getLogs",
            "eth_estimateGas",
            "eth_getStorageAt",
            "eth_gasPrice",
            "eth_getTransactionCount",
            "eth_sendRawTransaction",
        ]
        .contains(&method)
        {
            return Err(ApiError::bad("Unsupported BNB RPC method."));
        }
        let url = if method == "eth_getLogs" {
            self.config.logs_rpc.as_deref().unwrap_or(&self.config.rpc)
        } else {
            &self.config.rpc
        };
        if method == "eth_getLogs" && url != self.config.rpc {
            let chain = self.rpc_url(url, "eth_chainId", json!([])).await?;
            if chain != "0x38" {
                return Err(ApiError::unavailable("BNB logs RPC chain mismatch."));
            }
        }
        self.rpc_url(url, method, params).await
    }
    pub(crate) async fn rpc_url(&self, url: &str, method: &str, params: Value) -> Result<Value> {
        // A named client is accepted by providers that reject anonymous library
        // traffic. Retry only transient reads, never a transaction broadcast.
        let retryable_read = method != "eth_sendRawTransaction";
        for attempt in 0..2 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            let response = self
                .client
                .post(url)
                .header(
                    reqwest::header::USER_AGENT,
                    "tabagents/0.2 (+https://tabagents.io)",
                )
                .timeout(Duration::from_secs(15))
                .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(_) if retryable_read && attempt == 0 => continue,
                Err(_) => {
                    return Err(ApiError::unavailable(
                        "BNB chain data is temporarily unavailable.",
                    ))
                }
            };
            if retryable_read
                && attempt == 0
                && matches!(response.status().as_u16(), 429 | 502 | 503 | 504)
            {
                continue;
            }
            let body: Value = response
                .error_for_status()
                .map_err(|_| ApiError::unavailable("BNB chain data is temporarily unavailable."))?
                .json()
                .await
                .map_err(|_| ApiError::unavailable("Invalid BNB RPC response."))?;
            if let Some(error) = body.get("error").filter(|error| !error.is_null()) {
                if retryable_read
                    && attempt == 0
                    && matches!(error["code"].as_i64(), Some(429 | -32005 | -32029))
                {
                    continue;
                }
                return Err(ApiError::unavailable(
                    "BNB RPC could not verify this request.",
                ));
            }
            return body
                .get("result")
                .cloned()
                .ok_or_else(|| ApiError::unavailable("Invalid BNB RPC response."));
        }
        Err(ApiError::unavailable(
            "BNB chain data is temporarily unavailable.",
        ))
    }
    pub async fn require_network(&self) -> Result<()> {
        if self.config.chain_id != CHAIN_ID
            || self.rpc("eth_chainId", json!([])).await? != json!("0x38")
        {
            return Err(ApiError::unavailable(
                "The RPC is not BNB Smart Chain mainnet.",
            ));
        }
        Ok(())
    }
    pub fn manifest(&self) -> Result<Value> {
        let bytes = std::fs::read(&self.config.manifest)
            .map_err(|_| ApiError::unavailable("Deployment manifest is unavailable."))?;
        serde_json::from_slice(&bytes)
            .map_err(|_| ApiError::unavailable("Invalid deployment manifest."))
    }
    pub fn module(&self, module: &str) -> Result<String> {
        if module == "protocol" {
            return address(&self.config.program);
        }
        address(
            self.manifest()?["contracts"][module]["address"]
                .as_str()
                .ok_or_else(|| ApiError::unavailable("Contract module is not configured."))?,
        )
    }
    pub fn instruction(&self, module: &str, method: &str, args: Vec<Token>) -> Result<Instruction> {
        let contract = abi(module)?;
        let f = contract
            .function(method)
            .map_err(|_| ApiError::internal())?;
        let data = f
            .encode_input(&args)
            .map_err(|_| ApiError::validation("Invalid contract arguments."))?;
        Ok(Instruction {
            to: self.module(module)?,
            data: format!("0x{}", hex::encode(data)),
            value: "0x0".into(),
        })
    }
    pub async fn view(&self, module: &str, method: &str, args: Vec<Token>) -> Result<Value> {
        self.view_at(module, method, args, "latest").await
    }
    pub async fn view_at(
        &self,
        module: &str,
        method: &str,
        args: Vec<Token>,
        block: &str,
    ) -> Result<Value> {
        let ix = self.instruction(module, method, args)?;
        let value = self
            .rpc("eth_call", json!([{"to":ix.to,"data":ix.data},block]))
            .await?;
        let bytes = hex::decode(
            value
                .as_str()
                .and_then(|v| v.strip_prefix("0x"))
                .ok_or_else(|| ApiError::unavailable("Invalid contract state."))?,
        )
        .map_err(|_| ApiError::unavailable("Invalid contract state."))?;
        let contract = abi(module)?;
        let f = contract
            .function(method)
            .map_err(|_| ApiError::internal())?;
        let result = f
            .decode_output(&bytes)
            .map_err(|_| ApiError::unavailable("Contract ABI does not match the deployed code."))?;
        let spec: Value = serde_json::from_str(abi_json(module)?)?;
        let function = spec
            .as_array()
            .and_then(|a| {
                a.iter()
                    .find(|v| v["name"] == method && v["type"] == "function")
            })
            .ok_or_else(ApiError::internal)?;
        if result.len() == 1 {
            return Ok(token_json(&result[0], &function["outputs"][0]));
        }
        if function["outputs"].as_array().is_some_and(|outputs| outputs.iter().all(|output|
            output["name"].as_str().is_some_and(|name| !name.is_empty()))) {
            let values: serde_json::Map<String, Value> = result.iter().enumerate().map(|(i, token)| {
                let output = &function["outputs"][i];
                (output["name"].as_str().unwrap().to_owned(), token_json(token, output))
            }).collect();
            return Ok(Value::Object(values));
        }
        Ok(json!(result
            .iter()
            .enumerate()
            .map(|(i, t)| token_json(t, &function["outputs"][i]))
            .collect::<Vec<_>>()))
    }
    pub async fn erc20(&self, token: &str, signature: &str, args: Vec<Token>) -> Result<U256> {
        let mut data = Keccak256::digest(signature.as_bytes())[..4].to_vec();
        data.extend(ethabi::encode(&args));
        let v = self
            .rpc(
                "eth_call",
                json!([{"to":address(token)?,"data":format!("0x{}",hex::encode(data))},"latest"]),
            )
            .await?;
        let raw = v
            .as_str()
            .and_then(|v| v.strip_prefix("0x"))
            .ok_or_else(|| ApiError::unavailable("Invalid ERC20 response."))?;
        if raw.len() != 64 {
            return Err(ApiError::unavailable("Invalid ERC20 response width."));
        }
        U256::from_str_radix(raw, 16).map_err(|_| ApiError::unavailable("Invalid ERC20 integer."))
    }
    pub async fn token_decimals(&self, token: &str) -> Result<u8> {
        let value = self.erc20(token, "decimals()", vec![]).await?;
        if value > U256::from(18) {
            return Err(ApiError::validation(
                "Token precision above 18 is unsupported.",
            ));
        }
        Ok(value.low_u32() as u8)
    }
    pub async fn token_balance(&self, wallet: &str, token: &str) -> Result<(u128, u8)> {
        let decimals = self.token_decimals(token).await?;
        let value = self
            .erc20(token, "balanceOf(address)", vec![addr(wallet)?])
            .await?;
        Ok((number(&json!(value.to_string()))?, decimals))
    }
    pub async fn balances(&self, wallet: &str) -> Result<Value> {
        self.require_network().await?;
        let wallet = address(wallet)?;
        let bnb = number(
            &self
                .rpc("eth_getBalance", json!([wallet, "latest"]))
                .await?,
        )?;
        let (tokens, decimals) = self.token_balance(&wallet, &self.config.usdt).await?;
        if decimals != 18 {
            return Err(ApiError::unavailable("Unexpected USDT precision."));
        }
        Ok(
            json!({"wallet":wallet,"bnb":crate::models::money(bnb).to_string(),"usdt":crate::models::money(tokens).to_string()}),
        )
    }
    pub async fn verify_deployment(&self) -> Result<bool> {
        if self.config.program.is_empty() || !self.config.manifest.exists() {
            return Ok(false);
        }
        self.require_network().await?;
        let doc = self.manifest()?;
        if doc["chain_id"] != CHAIN_ID
            || doc["usdt_decimals"] != 18
            || doc["usdt_address"]
                .as_str()
                .and_then(|a| address(a).ok())
                .as_deref()
                != Some(USDT)
            || doc["contracts"]["protocol"]["address"]
                .as_str()
                .and_then(|a| address(a).ok())
                .as_deref()
                != Some(self.config.program.as_str())
        {
            return Ok(false);
        }
        for module in ["protocol", "backing", "economics"] {
            let a = self.module(module)?;
            let code = self.rpc("eth_getCode", json!([a, "latest"])).await?;
            let bytes = hex::decode(code.as_str().unwrap_or("").trim_start_matches("0x"))
                .map_err(|_| ApiError::unavailable("Invalid contract bytecode."))?;
            if bytes.is_empty() || doc["contracts"][module]["code_hash"] != hash(&bytes) {
                return Ok(false);
            }
        }
        let usdt = self.rpc("eth_getCode", json!([USDT, "latest"])).await?;
        let bytes = hex::decode(usdt.as_str().unwrap_or("").trim_start_matches("0x"))
            .map_err(|_| ApiError::unavailable("Invalid token bytecode."))?;
        if bytes.is_empty()
            || doc["usdt_code_hash"] != hash(&bytes)
            || self.token_decimals(USDT).await? != 18
        {
            return Ok(false);
        }
        let p = self.view("protocol", "getProtocol", vec![]).await?;
        if p["usdt"] != USDT
            || p["backing"] != self.module("backing")?
            || p["economics"] != self.module("economics")?
            || number(&p["feeBps"])? != doc["fee_bps"].as_u64().unwrap_or(200) as u128
        {
            return Ok(false);
        }
        if let Some(previous) = doc["previous_protocol"].as_str() {
            if self.view("protocol", "legacyProtocol", vec![]).await?.as_str() != Some(previous) {
                return Err(ApiError::unavailable("Migration registry differs from the deployment manifest."));
            }
        }
        for module in ["backing", "economics"] {
            if self.view(module, "protocol", vec![]).await? != self.config.program
                || self.view(module, "usdt", vec![]).await? != USDT
            {
                return Ok(false);
            }
        }
        if p["authority"].as_str().and_then(|a| address(a).ok())
            != doc["authority"].as_str().and_then(|a| address(a).ok())
            || doc["authority"].is_null()
        {
            return Ok(false);
        }
        if let Some(tab) = &self.config.official_tab {
            if p["tabToken"] != *tab || doc["official_tab_address"] != *tab {
                return Ok(false);
            }
        } else if p["tabToken"] != ZERO || !doc["official_tab_address"].is_null() {
            return Ok(false);
        }
        Ok(true)
    }
    pub async fn verify_backing_asset(&self, asset: &Value) -> Result<()> {
        let token = asset["address"]
            .as_str()
            .ok_or_else(|| ApiError::validation("Token address is unavailable."))?;
        if address(token)? == self.config.usdt {
            return self.require_deployment().await;
        }
        if asset["chain_id"] != 56
            || self.token_decimals(token).await? != asset["decimals"].as_u64().unwrap_or(255) as u8
        {
            return Err(ApiError::unavailable(
                "Backing token network or precision changed.",
            ));
        }
        for (address_key, hash_key) in [
            ("address", "code_hash"),
            ("beacon_address", "beacon_code_hash"),
            ("implementation_address", "implementation_code_hash"),
        ] {
            let addr = asset[address_key].as_str().ok_or_else(|| {
                ApiError::unavailable("Backing token verification is incomplete.")
            })?;
            let code = self.rpc("eth_getCode", json!([addr, "latest"])).await?;
            let bytes = hex::decode(code.as_str().unwrap_or("").trim_start_matches("0x"))
                .map_err(|_| ApiError::unavailable("Invalid backing token bytecode."))?;
            if bytes.is_empty() || asset[hash_key] != hash(&bytes) {
                return Err(ApiError::unavailable(
                    "Backing token code changed; custody needs re-verification.",
                ));
            }
        }
        let slot = self
            .rpc(
                "eth_getStorageAt",
                json!([
                    token,
                    "0xa3f0ad74e5423aebfd80d3ef4346578335a9a72aeaee59ff6cb3582b35133d50",
                    "latest"
                ]),
            )
            .await?;
        let expected = format!(
            "0x{:0>64}",
            asset["beacon_address"]
                .as_str()
                .unwrap_or("")
                .trim_start_matches("0x")
        );
        if slot
            .as_str()
            .is_none_or(|s| !s.eq_ignore_ascii_case(&expected))
        {
            return Err(ApiError::unavailable("Backing token beacon changed."));
        }
        let implementation = self
            .rpc(
                "eth_call",
                json!([{"to":asset["beacon_address"],"data":"0x5c60da1b"},"latest"]),
            )
            .await?;
        let expected = format!(
            "0x{:0>64}",
            asset["implementation_address"]
                .as_str()
                .unwrap_or("")
                .trim_start_matches("0x")
        );
        if implementation
            .as_str()
            .is_none_or(|s| !s.eq_ignore_ascii_case(&expected))
        {
            return Err(ApiError::unavailable(
                "Backing token implementation changed.",
            ));
        }
        Ok(())
    }
    pub async fn deployed(&self) -> bool {
        let mut cache = self.deployment.lock().await;
        if let Some((at, value)) = &*cache {
            if at.elapsed() < Duration::from_secs(15) {
                return *value;
            }
        }
        let result = self.verify_deployment().await.unwrap_or(false);
        *cache = Some((Instant::now(), result));
        result
    }
    pub async fn require_deployment(&self) -> Result<()> {
        if !self.verify_deployment().await? {
            return Err(ApiError::unavailable("The BNB deployment, contract bytecode and USDT configuration could not be verified."));
        }
        Ok(())
    }
    pub fn agent_address(&self, wallet: &str, id_value: &str) -> Result<String> {
        let wallet = address(wallet)?;
        let raw = id_value.trim_start_matches("0x");
        if raw.len() != 32 || hex::decode(raw).is_err() {
            return Err(ApiError::validation("Invalid local agent identifier."));
        }
        Ok(format!("{}{}", wallet, &raw[8..]))
    }
    pub fn job_address(&self, id_value: &str) -> Result<String> {
        id(id_value)
    }
    pub async fn agent(&self, id_value: &str) -> Result<Value> {
        self.view("protocol", "getAgent", vec![bytes32(id_value)?])
            .await
    }
    pub async fn job(&self, id_value: &str) -> Result<Value> {
        self.view("protocol", "getJob", vec![bytes32(id_value)?])
            .await
    }
    pub async fn transactions(
        &self,
        sender: &str,
        ix: &Instruction,
        approval: Option<(&str, u128)>,
    ) -> Result<Vec<EvmTransaction>> {
        let mut txs = vec![];
        if let Some((token, amount)) = approval {
            let spender = addr(&ix.to)?;
            let allowance = self
                .erc20(
                    token,
                    "allowance(address,address)",
                    vec![addr(sender)?, spender.clone()],
                )
                .await?;
            if allowance < U256::from(amount) {
                for value in if allowance.is_zero() {
                    vec![amount]
                } else {
                    vec![0, amount]
                } {
                    let mut data = Keccak256::digest(b"approve(address,uint256)")[..4].to_vec();
                    data.extend(ethabi::encode(&[spender.clone(), uint(value)]));
                    txs.push(EvmTransaction {
                        to: address(token)?,
                        data: format!("0x{}", hex::encode(data)),
                        value: "0x0".into(),
                        chain_id: "0x38".into(),
                    });
                }
            }
        }
        if txs.is_empty() {
            self.rpc(
                "eth_estimateGas",
                json!([{"from":address(sender)?,"to":ix.to,"data":ix.data,"value":ix.value}]),
            )
            .await?;
        }
        txs.push(ix.transaction());
        Ok(txs)
    }
    pub async fn transaction_status(&self, tx_hash: &str) -> Result<Value> {
        signature(tx_hash)?;
        self.require_network().await?;
        let receipt = self
            .rpc("eth_getTransactionReceipt", json!([tx_hash]))
            .await?;
        if receipt.is_null() {
            return Ok(json!({"status":"pending","confirmations":0}));
        }
        if receipt["transactionHash"]
            .as_str()
            .is_none_or(|h| !h.eq_ignore_ascii_case(tx_hash))
        {
            return Err(ApiError::unavailable("Transaction receipt hash mismatch."));
        }
        if receipt["status"] != "0x0" && receipt["status"] != "0x1" {
            return Err(ApiError::unavailable(
                "Transaction success is not confirmed.",
            ));
        }
        let block = number(&receipt["blockNumber"])?;
        let head = number(&self.rpc("eth_blockNumber", json!([])).await?)?;
        let confirmations = head.checked_sub(block).map(|n| n + 1).unwrap_or(0);
        let canonical = self
            .rpc(
                "eth_getBlockByNumber",
                json!([receipt["blockNumber"], false]),
            )
            .await?;
        if canonical["hash"] != receipt["blockHash"] || canonical["hash"].is_null() {
            return Err(ApiError::unavailable(
                "The transaction block is no longer canonical.",
            ));
        }
        Ok(
            json!({"status":if confirmations<u128::from(self.config.confirmations){"pending"}else if receipt["status"]=="0x0"{"failed"}else{"confirmed"},"confirmations":confirmations,"receipt":receipt}),
        )
    }
    pub async fn verify_envelope(
        &self,
        tx_hash: &str,
        sender: &str,
        ix: &Instruction,
    ) -> Result<Value> {
        signature(tx_hash)?;
        self.require_deployment().await?;
        let tx = self
            .rpc("eth_getTransactionByHash", json!([tx_hash]))
            .await?;
        if tx.is_null() {
            return Err(ApiError::conflict(
                "The RPC has not indexed this submitted transaction yet. Retry shortly.",
            ));
        }
        if tx["hash"]
            .as_str()
            .is_none_or(|h| !h.eq_ignore_ascii_case(tx_hash))
            || tx["chainId"] != "0x38"
            || tx["from"].as_str().and_then(|s| address(s).ok()) != Some(address(sender)?)
            || tx["to"].as_str().and_then(|s| address(s).ok()) != Some(address(&ix.to)?)
            || tx["input"]
                .as_str()
                .is_none_or(|s| !s.eq_ignore_ascii_case(&ix.data))
            || number(&tx["value"])? != number(&json!(ix.value))?
        {
            return Err(ApiError::bad(
                "Submitted transaction does not match this exact wallet action.",
            ));
        }
        Ok(tx)
    }
    pub async fn verify_transaction(
        &self,
        tx_hash: &str,
        sender: &str,
        ix: &Instruction,
    ) -> Result<Value> {
        self.require_deployment().await?;
        let state = self.transaction_status(tx_hash).await?;
        if state["status"] == "failed" {
            return Err(ApiError::bad("The BNB transaction reverted."));
        }
        if state["status"] != "confirmed" {
            return Err(ApiError::conflict(
                "Waiting for BNB transaction confirmations.",
            ));
        }
        let tx = self
            .rpc("eth_getTransactionByHash", json!([tx_hash]))
            .await?;
        let receipt = &state["receipt"];
        if tx["hash"]
            .as_str()
            .is_none_or(|h| !h.eq_ignore_ascii_case(tx_hash))
            || tx["chainId"] != "0x38"
            || tx["from"].as_str().and_then(|s| address(s).ok()) != Some(address(sender)?)
            || tx["to"].as_str().and_then(|s| address(s).ok()) != Some(address(&ix.to)?)
            || tx["input"]
                .as_str()
                .is_none_or(|s| !s.eq_ignore_ascii_case(&ix.data))
            || number(&tx["value"])? != number(&json!(ix.value))?
            || tx["blockHash"] != receipt["blockHash"]
            || tx["blockNumber"] != receipt["blockNumber"]
            || receipt["to"].as_str().and_then(|s| address(s).ok()) != Some(address(&ix.to)?)
        {
            return Err(ApiError::bad("Transaction does not match the exact signer, destination, calldata, value and chain of this action."));
        }
        Ok(receipt.clone())
    }
}
