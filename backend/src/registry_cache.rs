//! Durable discovery of confirmed registrations; never a source of spending authority.
use crate::{
    bnb::{self, Bnb},
    db::Store,
    error::{ApiError, Result},
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

const CHUNK: u64 = 2000;
// Bound a public refresh even after downtime. Each complete chunk is durable,
// so cancellation or a later RPC failure cannot make successful work repeat.
const MAX_CHUNKS_PER_REFRESH: u64 = 8;
const SCAN_LOCK_TIMEOUT: Duration = Duration::from_secs(2);
const SCAN_TIMEOUT: Duration = Duration::from_secs(8);
const DATABASE_BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_AGENTS: usize = 10_000;
const REGISTERED: &[u8] = b"AgentRegistered(bytes32,address,string,uint256,bytes32)";
const CATCHING_UP: &str = "Confirmed registry history is catching up. Refresh shortly.";
const STALE_AFTER_SECONDS: i64 = 90;

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RegistryProgress {
    pub status: String,
    pub deployment_block: Option<String>,
    pub scanned_through_block: Option<String>,
    pub confirmed_head_block: Option<String>,
    pub remaining_blocks: Option<String>,
    pub verified_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct RegistrySnapshot {
    pub data: crate::models::RegistryData,
    pub generation: u64,
}

#[derive(Default)]
pub(crate) struct RefreshState {
    start: Option<u64>,
    head: Option<u64>,
    verified: Option<Checkpoint>,
    verified_at: Option<chrono::DateTime<chrono::Utc>>,
    failed: bool,
    invalidated: bool,
    generation: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Checkpoint {
    start: u64,
    block: u64,
    hash: String,
    ids: BTreeSet<String>,
}
fn failure() -> ApiError {
    ApiError::unavailable("Confirmed BNB registry history is unavailable.")
}
fn block_number(value: &Value) -> Result<u64> {
    bnb::number(value)?.try_into().map_err(|_| failure())
}
fn hash(value: &Value) -> Result<String> {
    let value = value.as_str().ok_or_else(failure)?;
    bnb::signature(value).map_err(|_| failure())?;
    Ok(value.to_lowercase())
}
fn checkpoint(store: &Store, protocol: &str) -> Result<Option<Checkpoint>> {
    let db = store.connect()?;
    db.busy_timeout(DATABASE_BUSY_TIMEOUT)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS registry_checkpoints(protocol TEXT PRIMARY KEY,chain_id INTEGER NOT NULL CHECK(chain_id=56),payload TEXT NOT NULL);")?;
    read_checkpoint(store, protocol)
}
fn read_checkpoint(store: &Store, protocol: &str) -> Result<Option<Checkpoint>> {
    let db = store.connect()?;
    db.busy_timeout(DATABASE_BUSY_TIMEOUT)?;
    let exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='registry_checkpoints')", [], |row| row.get(0))?;
    if !exists {
        return Ok(None);
    }
    let row: Option<String> = db
        .query_row(
            "SELECT payload FROM registry_checkpoints WHERE protocol=? AND chain_id=56",
            [protocol],
            |r| r.get(0),
        )
        .optional()?;
    row.map(|raw| serde_json::from_str(&raw).map_err(|_| failure()))
        .transpose()
}
fn persist(
    store: &Store,
    protocol: &str,
    previous: &Option<Checkpoint>,
    next: &Checkpoint,
) -> Result<()> {
    let mut db = store.connect()?;
    db.busy_timeout(DATABASE_BUSY_TIMEOUT)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let row: Option<String> = tx
        .query_row(
            "SELECT payload FROM registry_checkpoints WHERE protocol=? AND chain_id=56",
            [protocol],
            |r| r.get(0),
        )
        .optional()?;
    let current: Option<Checkpoint> = row
        .map(|raw| serde_json::from_str(&raw).map_err(|_| failure()))
        .transpose()?;
    // A second process must not overwrite a newer checkpoint after a slow scan.
    if &current != previous {
        return Err(ApiError::unavailable(
            "Registry refresh changed; retry this request.",
        ));
    }
    tx.execute("INSERT INTO registry_checkpoints(protocol,chain_id,payload) VALUES(?,56,?) ON CONFLICT(protocol) DO UPDATE SET chain_id=56,payload=excluded.payload",params![protocol,serde_json::to_string(next)?])?;
    tx.commit()?;
    Ok(())
}
impl Bnb {
    /// Read-only discovery metadata, independent of RPC availability. Persisted
    /// progress is visible after restart, but IDs remain private until this
    /// process has checked the saved checkpoint against the canonical chain.
    pub fn registry_progress(&self, store: &Store) -> RegistryProgress {
        let state = self
            .registry_state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let start = state
            .start
            .or_else(|| self.manifest().ok()?["deployment_block"].as_u64());
        let saved = if state.invalidated {
            None
        } else {
            state.verified.clone().or_else(|| {
                read_checkpoint(store, &self.module("protocol").ok()?)
                    .ok()
                    .flatten()
                    .filter(|saved| Some(saved.start) == start && saved.block >= saved.start)
            })
        };
        let scanned = saved.as_ref().map(|saved| saved.block);
        let remaining = scanned
            .zip(state.head)
            .map(|(block, head)| head.saturating_sub(block));
        let aged = state
            .verified_at
            .is_some_and(|at| (chrono::Utc::now() - at).num_seconds() > STALE_AFTER_SECONDS);
        let status = if state.verified.is_none() {
            if state.failed {
                "unavailable"
            } else {
                "checking"
            }
        } else if state.failed || aged {
            "stale"
        } else if remaining.is_some_and(|blocks| blocks > 0) {
            "catching_up"
        } else {
            "live"
        };
        RegistryProgress {
            status: status.into(),
            deployment_block: start.map(|value| value.to_string()),
            scanned_through_block: scanned.map(|value| value.to_string()),
            confirmed_head_block: state.head.map(|value| value.to_string()),
            remaining_blocks: remaining.map(|value| value.to_string()),
            verified_at: state.verified_at.map(|at| at.to_rfc3339()),
            error: matches!(status, "unavailable" | "stale").then(|| {
                "BNB discovery is reconnecting. Confirmed records will refresh automatically."
                    .into()
            }),
        }
    }
    pub fn cached_registered_ids(&self) -> Option<(BTreeSet<String>, u64)> {
        let state = self
            .registry_state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state
            .verified
            .as_ref()
            .map(|saved| (saved.ids.clone(), saved.block))
    }
    pub(crate) fn registry_generation(&self) -> u64 {
        self.registry_state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .generation
    }
    pub async fn verify_registry_block(&self, block: u64) -> Result<()> {
        let expected = {
            let state = self
                .registry_state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state
                .verified
                .as_ref()
                .filter(|saved| saved.block == block)
                .map(|saved| saved.hash.clone())
                .ok_or_else(failure)?
        };
        if self.registry_block_hash(block).await? != expected {
            self.registry_invalidated();
            return Err(failure());
        }
        Ok(())
    }
    fn registry_verified(&self, saved: &Checkpoint) {
        let mut state = self
            .registry_state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.verified = Some(saved.clone());
        state.verified_at = Some(chrono::Utc::now());
        state.invalidated = false;
    }
    fn registry_invalidated(&self) {
        let mut state = self
            .registry_state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.verified = None;
        state.verified_at = None;
        if !state.invalidated {
            state.generation = state.generation.wrapping_add(1);
        }
        state.invalidated = true;
    }
    async fn registry_logs_anchor(&self, block: u64, expected: &str) -> Result<()> {
        let Some(url) = self
            .config
            .logs_rpc
            .as_deref()
            .filter(|url| *url != self.config.rpc)
        else {
            return Ok(());
        };
        for (method, parameters) in [
            ("eth_chainId", json!([])),
            (
                "eth_getBlockByNumber",
                json!([format!("0x{block:x}"), false]),
            ),
        ] {
            let result = self.rpc_url(url, method, parameters).await?;
            if method == "eth_chainId" {
                if result != "0x38" {
                    return Err(failure());
                }
            } else if block_number(&result["number"])? != block
                || hash(&result["hash"])? != expected
            {
                return Err(failure());
            }
        }
        Ok(())
    }
    async fn registry_block_hash(&self, block: u64) -> Result<String> {
        let value = self
            .rpc(
                "eth_getBlockByNumber",
                json!([format!("0x{block:x}"), false]),
            )
            .await?;
        if block_number(&value["number"])? != block {
            return Err(failure());
        }
        hash(&value["hash"])
    }
    pub async fn registered_ids(&self, store: &Store) -> Result<(BTreeSet<String>, u64)> {
        // Public callers must not queue behind several historical scans. Timing
        // out drops this lock request, rather than leaving a waiter in the queue.
        let _guard = tokio::time::timeout(SCAN_LOCK_TIMEOUT, self.registry_scan.lock())
            .await
            .map_err(|_| ApiError::unavailable("Registry refresh is busy. Retry shortly."))?;
        // Cancellation drops the in-flight RPC future and releases the guard.
        // Only chunks already committed after every canonical check survive.
        let result = tokio::time::timeout(SCAN_TIMEOUT, self.scan_registered_ids(store))
            .await
            .map_err(|_| ApiError::unavailable("Registry refresh timed out. Retry shortly."))
            .and_then(|result| result);
        self.registry_state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .failed = result.as_ref().is_err_and(|error| error.1 != CATCHING_UP);
        result
    }
    async fn scan_registered_ids(&self, store: &Store) -> Result<(BTreeSet<String>, u64)> {
        self.require_network().await?;
        let protocol = self.module("protocol")?;
        let start = self.manifest()?["deployment_block"]
            .as_u64()
            .ok_or_else(failure)?;
        let head = block_number(&self.rpc("eth_blockNumber", json!([])).await?)?;
        let confirmed = head.saturating_sub(u64::from(self.config.confirmations.saturating_sub(1)));
        {
            let mut state = self
                .registry_state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.start = Some(start);
            state.head = Some(confirmed);
        }
        if confirmed < start {
            self.registry_invalidated();
            return Err(failure());
        }
        let mut previous = checkpoint(store, &protocol)?;
        let mut ids = BTreeSet::new();
        let mut from = start;
        let mut reused = None;
        if let Some(saved) = &previous {
            if saved.start == start
                && saved.block >= start
                && saved.block <= confirmed
                && self.registry_block_hash(saved.block).await? == saved.hash
            {
                if saved.ids.len() > MAX_AGENTS
                    || saved.ids.iter().any(|id| bnb::signature(id).is_err())
                {
                    self.registry_invalidated();
                    return Err(failure());
                }
                self.registry_verified(saved);
                ids = saved.ids.clone();
                if saved.block == confirmed {
                    return Ok((ids, confirmed));
                }
                from = saved.block.checked_add(1).ok_or_else(failure)?;
                reused = Some((saved.block, saved.hash.clone()));
            } else {
                self.registry_invalidated();
            }
        }
        let topic = bnb::hash(REGISTERED);
        let mut block_hashes: BTreeMap<u64, String> = BTreeMap::new();
        for _ in 0..MAX_CHUNKS_PER_REFRESH {
            let to = from.saturating_add(CHUNK - 1).min(confirmed);
            let end_hash = self.registry_block_hash(to).await?;
            // Empty ranges are complete only if both RPCs agree on the range's
            // canonical end block before and after the complete logs response.
            self.registry_logs_anchor(to, &end_hash).await?;
            let logs = self.rpc("eth_getLogs",json!([{"address":protocol,"fromBlock":format!("0x{from:x}"),"toBlock":format!("0x{to:x}"),"topics":[topic]}])).await?;
            for log in logs.as_array().ok_or_else(failure)? {
                let block = block_number(&log["blockNumber"])?;
                let topics = log["topics"].as_array().ok_or_else(failure)?;
                if bnb::address(log["address"].as_str().ok_or_else(failure)?)? != protocol
                    || log["removed"] != false
                    || block < from
                    || block > to
                    || topics.len() != 3
                    || hash(&topics[0])? != topic
                {
                    return Err(failure());
                }
                let id = hash(&topics[1])?;
                hash(&topics[2])?;
                let event_hash = hash(&log["blockHash"])?;
                let canonical = if let Some(value) = block_hashes.get(&block) {
                    value.clone()
                } else {
                    let value = self.registry_block_hash(block).await?;
                    block_hashes.insert(block, value.clone());
                    value
                };
                if event_hash != canonical {
                    return Err(failure());
                }
                ids.insert(id);
                if ids.len() > MAX_AGENTS {
                    return Err(ApiError::unavailable("Registry pagination is required."));
                }
            }
            self.registry_logs_anchor(to, &end_hash).await?;
            if self.registry_block_hash(to).await? != end_hash {
                return Err(failure());
            }
            if let Some((block, hash)) = &reused {
                if self.registry_block_hash(*block).await? != *hash {
                    self.registry_invalidated();
                    return Err(failure());
                }
            }
            let next = Checkpoint {
                start,
                block: to,
                hash: end_hash.clone(),
                ids: ids.clone(),
            };
            persist(store, &protocol, &previous, &next)?;
            self.registry_verified(&next);
            previous = Some(next);
            reused = Some((to, end_hash));
            if to == confirmed {
                return Ok((ids, confirmed));
            }
            from = to.checked_add(1).ok_or_else(failure)?;
        }
        Err(ApiError::unavailable(CATCHING_UP))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
    use std::sync::{Arc, Mutex};

    const PROTOCOL: &str = "0x1111111111111111111111111111111111111111";
    fn block_hash(block: u64) -> String {
        format!("0x{block:064x}")
    }
    fn id(n: u64) -> String {
        format!("0x{n:064x}")
    }
    fn registration(block: u64, n: u64) -> Value {
        json!({"address":PROTOCOL,"blockNumber":format!("0x{block:x}"),"blockHash":block_hash(block),"removed":false,"topics":[bnb::hash(REGISTERED),id(n),format!("0x{:0>64}",&PROTOCOL[2..])]})
    }
    struct Mock {
        head: u64,
        hashes: BTreeMap<u64, String>,
        logs: Vec<Value>,
        fail_from: Option<u64>,
        ranges: Vec<(u64, u64)>,
        return_all_logs: bool,
        reorg_on_block: Option<(u64, u64)>,
        reorg_after_logs: Option<u64>,
        slow_logs_from: Option<u64>,
        slow_logs_started: Arc<tokio::sync::Notify>,
    }
    type Shared = Arc<Mutex<Mock>>;
    async fn rpc(
        State(shared): State<Shared>,
        headers: HeaderMap,
        Json(request): Json<Value>,
    ) -> Json<Value> {
        assert_eq!(
            headers.get("user-agent").unwrap(),
            "tabagents/0.2 (+https://tabagents.io)"
        );
        let delayed = {
            let state = shared.lock().unwrap();
            (request["method"] == "eth_getLogs"
                && state.slow_logs_from.is_some()
                && block_number(&request["params"][0]["fromBlock"]).ok() == state.slow_logs_from)
                .then(|| state.slow_logs_started.clone())
        };
        if let Some(started) = delayed {
            started.notify_one();
            tokio::time::sleep(SCAN_TIMEOUT + Duration::from_secs(5)).await;
        }
        let mut state = shared.lock().unwrap();
        let result = match request["method"].as_str().unwrap() {
            "eth_chainId" => json!("0x38"),
            "eth_blockNumber" => json!(format!("0x{:x}", state.head)),
            "eth_getBlockByNumber" => {
                let block = block_number(&request["params"][0]).unwrap();
                if block > state.head {
                    return Json(json!({"jsonrpc":"2.0","id":request["id"],"result":null}));
                }
                if let Some((trigger, checkpoint)) = state.reorg_on_block {
                    if block == trigger {
                        state.hashes.insert(checkpoint, block_hash(999));
                        state.reorg_on_block = None;
                    }
                }
                json!({"number":format!("0x{block:x}"),"hash":state.hashes.get(&block).cloned().unwrap_or_else(||block_hash(block))})
            }
            "eth_getLogs" => {
                let filter = &request["params"][0];
                let from = block_number(&filter["fromBlock"]).unwrap();
                let to = block_number(&filter["toBlock"]).unwrap();
                assert_eq!(filter["address"], PROTOCOL);
                assert_eq!(filter["topics"], json!([bnb::hash(REGISTERED)]));
                state.ranges.push((from, to));
                if state.fail_from == Some(from) {
                    return Json(
                        json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":"temporary log failure"}}),
                    );
                }
                if let Some(block) = state.reorg_after_logs.take() {
                    state.hashes.insert(block, block_hash(999));
                }
                json!(state
                    .logs
                    .iter()
                    .filter(|event| {
                        let block = block_number(&event["blockNumber"]).unwrap();
                        state.return_all_logs || (block >= from && block <= to)
                    })
                    .cloned()
                    .collect::<Vec<_>>())
            }
            _ => panic!("unexpected RPC method"),
        };
        Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
    }
    struct Fixture {
        _directory: tempfile::TempDir,
        bnb: Bnb,
        store: Store,
        mock: Shared,
        server: tokio::task::JoinHandle<()>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.server.abort();
        }
    }
    async fn fixture() -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let mock = Arc::new(Mutex::new(Mock {
            head: 12,
            hashes: BTreeMap::new(),
            logs: vec![registration(5, 1)],
            fail_from: None,
            ranges: vec![],
            return_all_logs: false,
            reorg_on_block: None,
            reorg_after_logs: None,
            slow_logs_from: None,
            slow_logs_started: Arc::new(tokio::sync::Notify::new()),
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rpc_url = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().route("/", post(rpc)).with_state(mock.clone());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let manifest = root.join("manifest.json");
        std::fs::write(&manifest, serde_json::to_vec(&json!({"chain_id":56,"deployment_block":1,"contracts":{"protocol":{"address":PROTOCOL}}})).unwrap()).unwrap();
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
            inference_daily_micros: 0,
            confirmations: 3,
        };
        let store = Store::open(&config.database).unwrap();
        let bnb = Bnb::new(Arc::new(config), reqwest::Client::new());
        Fixture {
            _directory: directory,
            bnb,
            store,
            mock,
            server,
        }
    }
    #[tokio::test]
    async fn same_confirmed_head_and_process_restart_do_not_rescan() {
        let f = fixture().await;
        let first = f.bnb.registered_ids(&f.store).await.unwrap();
        assert_eq!(first, (BTreeSet::from([id(1)]), 10));
        let (a, b) = tokio::join!(
            f.bnb.registered_ids(&f.store),
            f.bnb.registered_ids(&f.store)
        );
        assert_eq!(a.unwrap(), first);
        assert_eq!(b.unwrap(), first);
        let restarted = Bnb::new(f.bnb.config.clone(), f.bnb.client.clone());
        assert_eq!(restarted.registered_ids(&f.store).await.unwrap(), first);
        assert_eq!(f.mock.lock().unwrap().ranges, vec![(1, 10)]);
    }
    #[tokio::test]
    async fn public_registry_is_fast_during_catchup_and_clears_known_reorgs() {
        let f = fixture().await;
        let mut state = crate::AppState::new((*f.bnb.config).clone()).unwrap();
        state.bnb = f.bnb.clone();
        // Holding the scan lock cannot delay a public response or trigger RPC.
        let scan_lock = f.bnb.registry_scan.lock().await;
        let cold = tokio::time::timeout(Duration::from_millis(100), state.registry())
            .await
            .unwrap();
        assert_eq!(cold.status, "checking");
        assert!(cold.agents.is_empty());
        assert!(f.mock.lock().unwrap().ranges.is_empty());
        drop(scan_lock);
        f.bnb.registered_ids(&f.store).await.unwrap();
        let mut hydrated = cold;
        hydrated.owner = Some(PROTOCOL.into());
        hydrated.block_number = Some("10".into());
        hydrated.verified_at = Some(chrono::Utc::now().to_rfc3339());
        *state.registry_cache.write().await = Some(RegistrySnapshot {
            data: hydrated,
            generation: f.bnb.registry_generation(),
        });
        let live = state.registry().await;
        assert_eq!(live.status, "live");
        assert_eq!(live.block_number, Some("10".into()));
        {
            let mut mock = f.mock.lock().unwrap();
            mock.head = 15;
            mock.reorg_on_block = Some((13, 10));
        }
        assert!(f.bnb.registered_ids(&f.store).await.is_err());
        let invalidated = state.registry().await;
        assert_eq!(invalidated.status, "unavailable");
        assert!(invalidated.block_number.is_none());
        assert!(invalidated.owner.is_none());
        assert!(invalidated.verified_at.is_none());
        // A rebuilt canonical chunk must not resurrect the earlier hydrated
        // snapshot from the invalidated fork before new hydration finishes.
        f.mock.lock().unwrap().logs = vec![registration(6, 2)];
        assert_eq!(
            f.bnb.registered_ids(&f.store).await.unwrap().0,
            BTreeSet::from([id(2)])
        );
        let rebuilding = state.registry().await;
        assert_eq!(rebuilding.status, "checking");
        assert!(rebuilding.owner.is_none());
        assert!(rebuilding.block_number.is_none());
    }
    #[tokio::test]
    async fn transient_rpc_reads_retry_but_broadcasts_and_permanent_errors_do_not() {
        use axum::{http::StatusCode, response::IntoResponse};
        use std::sync::atomic::{AtomicUsize, Ordering};
        for (http_error, rpc_error, method, expected_calls, succeeds) in [
            (Some(503), None, "eth_chainId", 2, true),
            (Some(403), None, "eth_chainId", 1, false),
            (None, Some(-32005), "eth_chainId", 2, true),
            (Some(503), None, "eth_sendRawTransaction", 1, false),
        ] {
            let f = fixture().await;
            let calls = Arc::new(AtomicUsize::new(0));
            let counted = calls.clone();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let app = Router::new().route("/", post(move |headers: HeaderMap| {
                let calls = counted.clone();
                async move {
                    assert_eq!(headers.get("user-agent").unwrap(), "tabagents/0.2 (+https://tabagents.io)");
                    if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                        if let Some(code) = http_error {
                            return (StatusCode::from_u16(code).unwrap(), Json(json!({"error":"transient"}))).into_response();
                        }
                        if let Some(code) = rpc_error {
                            return Json(json!({"jsonrpc":"2.0","id":1,"error":{"code":code,"message":"rate limited"}})).into_response();
                        }
                    }
                    Json(json!({"jsonrpc":"2.0","id":1,"result":"0x38"})).into_response()
                }
            }));
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let mut config = (*f.bnb.config).clone();
            config.rpc = url;
            let bnb = Bnb::new(Arc::new(config), f.bnb.client.clone());
            assert_eq!(bnb.rpc(method, json!([])).await.is_ok(), succeeds);
            assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
            server.abort();
        }
    }
    #[tokio::test]
    async fn advancing_scans_only_new_confirmed_blocks() {
        let f = fixture().await;
        f.bnb.registered_ids(&f.store).await.unwrap();
        {
            let mut mock = f.mock.lock().unwrap();
            mock.head = 15;
            mock.logs.extend([registration(12, 2), registration(14, 3)]);
        }
        let (ids, block) = f.bnb.registered_ids(&f.store).await.unwrap();
        assert_eq!(block, 13);
        assert_eq!(ids, BTreeSet::from([id(1), id(2)]));
        assert_eq!(f.mock.lock().unwrap().ranges, vec![(1, 10), (11, 13)]);
    }
    #[tokio::test]
    async fn failed_later_chunk_keeps_only_completed_progress_without_skipping() {
        let f = fixture().await;
        f.bnb.registered_ids(&f.store).await.unwrap();
        {
            let mut mock = f.mock.lock().unwrap();
            mock.head = 2013;
            mock.logs
                .extend([registration(11, 2), registration(2011, 3)]);
            mock.fail_from = Some(2011);
        }
        assert!(f.bnb.registered_ids(&f.store).await.is_err());
        let saved = checkpoint(&f.store, PROTOCOL).unwrap().unwrap();
        assert_eq!(saved.block, 2010);
        assert_eq!(saved.ids, BTreeSet::from([id(1), id(2)]));
        f.mock.lock().unwrap().fail_from = None;
        assert_eq!(
            f.bnb.registered_ids(&f.store).await.unwrap(),
            (BTreeSet::from([id(1), id(2), id(3)]), 2011)
        );
        assert_eq!(
            f.mock.lock().unwrap().ranges,
            vec![(1, 10), (11, 2010), (2011, 2011), (2011, 2011)]
        );
    }
    #[tokio::test]
    async fn long_history_is_bounded_and_resumes_after_process_restart() {
        let f = fixture().await;
        let limit = CHUNK * MAX_CHUNKS_PER_REFRESH;
        {
            let mut mock = f.mock.lock().unwrap();
            mock.head = limit + 5;
            mock.logs.push(registration(limit + 1, 2));
        }
        assert!(f.bnb.registered_ids(&f.store).await.is_err());
        let saved = checkpoint(&f.store, PROTOCOL).unwrap().unwrap();
        assert_eq!(saved.block, limit);
        assert_eq!(saved.ids, BTreeSet::from([id(1)]));
        let progress = f.bnb.registry_progress(&f.store);
        assert_eq!(progress.status, "catching_up");
        assert_eq!(progress.scanned_through_block, Some(limit.to_string()));
        assert_eq!(progress.confirmed_head_block, Some((limit + 3).to_string()));
        assert_eq!(progress.remaining_blocks, Some("3".into()));
        assert!(progress.verified_at.is_some());
        assert!(progress.error.is_none());
        assert_eq!(
            f.mock.lock().unwrap().ranges.len(),
            MAX_CHUNKS_PER_REFRESH as usize
        );
        let restarted = Bnb::new(f.bnb.config.clone(), f.bnb.client.clone());
        let cold = restarted.registry_progress(&f.store);
        assert_eq!(cold.status, "checking");
        assert_eq!(cold.scanned_through_block, Some(limit.to_string()));
        assert!(cold.verified_at.is_none());
        assert!(restarted.cached_registered_ids().is_none());
        assert_eq!(
            restarted.registered_ids(&f.store).await.unwrap(),
            (BTreeSet::from([id(1), id(2)]), limit + 3)
        );
        assert_eq!(
            f.mock.lock().unwrap().ranges.last(),
            Some(&(limit + 1, limit + 3))
        );
        let progress = restarted.registry_progress(&f.store);
        assert_eq!(progress.status, "live");
        assert_eq!(progress.remaining_blocks, Some("0".into()));
    }
    #[tokio::test]
    async fn slow_scan_and_concurrent_waiters_time_out_without_losing_verified_progress() {
        let f = fixture().await;
        f.bnb.registered_ids(&f.store).await.unwrap();
        let started = {
            let mut mock = f.mock.lock().unwrap();
            mock.head = 2013;
            mock.logs
                .extend([registration(11, 2), registration(2011, 3)]);
            mock.slow_logs_from = Some(2011);
            mock.slow_logs_started.clone()
        };
        let bnb = f.bnb.clone();
        let store = f.store.clone();
        let scan = tokio::spawn(async move { bnb.registered_ids(&store).await });
        tokio::time::timeout(Duration::from_secs(3), started.notified())
            .await
            .unwrap();
        let before = tokio::time::Instant::now();
        let queued = f.bnb.registered_ids(&f.store).await.unwrap_err();
        assert!(queued.1.contains("busy"));
        assert!(before.elapsed() < SCAN_LOCK_TIMEOUT + Duration::from_secs(2));
        let timed_out = scan.await.unwrap().unwrap_err();
        assert!(timed_out.1.contains("timed out"));
        assert_eq!(f.bnb.registry_progress(&f.store).status, "stale");
        // The first range is complete, while the timed-out second range is
        // absent. No half-verified ID or future checkpoint became durable.
        let saved = checkpoint(&f.store, PROTOCOL).unwrap().unwrap();
        assert_eq!(saved.block, 2010);
        assert_eq!(saved.hash, block_hash(2010));
        assert_eq!(saved.ids, BTreeSet::from([id(1), id(2)]));
        assert!(f.bnb.registry_scan.try_lock().is_ok());
        f.mock.lock().unwrap().slow_logs_from = None;
        let recovered =
            tokio::time::timeout(Duration::from_secs(3), f.bnb.registered_ids(&f.store))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(recovered, (BTreeSet::from([id(1), id(2), id(3)]), 2011));
        assert_eq!(f.bnb.registry_progress(&f.store).status, "live");
        assert_eq!(
            f.mock.lock().unwrap().ranges,
            vec![(1, 10), (11, 2010), (2011, 2011)]
        );
    }
    #[tokio::test]
    async fn changed_checkpoint_hash_and_head_regression_rebuild_without_orphans() {
        let f = fixture().await;
        f.bnb.registered_ids(&f.store).await.unwrap();
        {
            let mut mock = f.mock.lock().unwrap();
            mock.hashes.insert(10, block_hash(999));
            mock.logs = vec![registration(6, 2)];
        }
        assert_eq!(
            f.bnb.registered_ids(&f.store).await.unwrap(),
            (BTreeSet::from([id(2)]), 10)
        );
        {
            let mut mock = f.mock.lock().unwrap();
            mock.head = 9;
            mock.logs = vec![registration(4, 3)];
        }
        assert_eq!(
            f.bnb.registered_ids(&f.store).await.unwrap(),
            (BTreeSet::from([id(3)]), 7)
        );
        assert_eq!(
            f.mock.lock().unwrap().ranges,
            vec![(1, 10), (1, 10), (1, 7)]
        );
    }
    #[tokio::test]
    async fn checkpoint_reorg_during_extension_does_not_commit_mixed_history() {
        let f = fixture().await;
        f.bnb.registered_ids(&f.store).await.unwrap();
        {
            let mut mock = f.mock.lock().unwrap();
            mock.head = 15;
            mock.reorg_on_block = Some((13, 10));
        }
        assert!(f.bnb.registered_ids(&f.store).await.is_err());
        let saved = checkpoint(&f.store, PROTOCOL).unwrap().unwrap();
        assert_eq!(saved.block, 10);
        assert_eq!(saved.hash, block_hash(10));
        assert!(f.bnb.cached_registered_ids().is_none());
        let progress = f.bnb.registry_progress(&f.store);
        assert_eq!(progress.status, "unavailable");
        assert!(progress.scanned_through_block.is_none());
        assert!(progress.verified_at.is_none());
    }
    #[tokio::test]
    async fn malformed_removed_wrong_contract_and_noncanonical_logs_fail_closed() {
        let f = fixture().await;
        f.mock.lock().unwrap().return_all_logs = true;
        for (field, value) in [
            (
                "address",
                json!("0x2222222222222222222222222222222222222222"),
            ),
            ("removed", json!(true)),
            ("blockNumber", json!("0xb")),
            ("blockHash", json!(block_hash(900))),
            ("topics", json!([id(0), id(1), id(2)])),
        ] {
            let mut event = registration(5, 1);
            event[field] = value;
            f.mock.lock().unwrap().logs = vec![event];
            assert!(
                f.bnb.registered_ids(&f.store).await.is_err(),
                "accepted bad {field}"
            );
            assert!(checkpoint(&f.store, PROTOCOL).unwrap().is_none());
        }
    }
    #[tokio::test]
    async fn separate_logs_provider_must_reach_the_canonical_end_block_before_commit() {
        let f = fixture().await;
        let logs = Arc::new(Mutex::new(Mock {
            head: 7,
            hashes: BTreeMap::new(),
            logs: vec![],
            fail_from: None,
            ranges: vec![],
            return_all_logs: false,
            reorg_on_block: None,
            reorg_after_logs: None,
            slow_logs_from: None,
            slow_logs_started: Arc::new(tokio::sync::Notify::new()),
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().route("/", post(rpc)).with_state(logs.clone());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut config = (*f.bnb.config).clone();
        config.logs_rpc = Some(url);
        let bnb = Bnb::new(Arc::new(config), f.bnb.client.clone());
        // A lagging provider can return an empty range without possessing the end block.
        assert!(bnb.registered_ids(&f.store).await.is_err());
        assert!(checkpoint(&f.store, PROTOCOL).unwrap().is_none());
        {
            let mut mock = logs.lock().unwrap();
            mock.head = 12;
            mock.hashes.insert(10, block_hash(999));
        }
        assert!(bnb.registered_ids(&f.store).await.is_err());
        assert!(checkpoint(&f.store, PROTOCOL).unwrap().is_none());
        assert!(logs.lock().unwrap().ranges.is_empty());
        {
            let mut mock = logs.lock().unwrap();
            mock.hashes.clear();
            mock.reorg_after_logs = Some(10);
        }
        assert!(bnb.registered_ids(&f.store).await.is_err());
        assert!(checkpoint(&f.store, PROTOCOL).unwrap().is_none());
        logs.lock().unwrap().hashes.clear();
        assert_eq!(
            bnb.registered_ids(&f.store).await.unwrap(),
            (BTreeSet::new(), 10)
        );
        assert_eq!(logs.lock().unwrap().ranges, vec![(1, 10), (1, 10)]);
        server.abort();
    }
}
