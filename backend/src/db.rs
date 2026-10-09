use crate::{
    error::{ApiError, Result},
    models::{now, AgentPlan, AgentRun, Job, RuntimeAgent},
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone)]
pub struct Store {
    pub path: PathBuf,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| ApiError::internal())?;
        }
        let store = Self { path: path.into() };
        let db = store.connect()?;
        let identity_exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='network_identity')",[],|r|r.get(0))?;
        if identity_exists {
            let chain: String =
                db.query_row("SELECT chain FROM network_identity LIMIT 1", [], |r| {
                    r.get(0)
                })?;
            if chain != "eip155:56:usdt18" {
                return Err(ApiError::unavailable(
                    "Database network mismatch. Use the separate BNB database.",
                ));
            }
        } else {
            let existing: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name IN ('runtime_agents','jobs','wallet_intents'))",[],|r|r.get(0))?;
            if existing {
                return Err(ApiError::unavailable("Existing chain records have no BNB identity. Preserve this database and choose a new BNB database."));
            }
            db.execute_batch("CREATE TABLE network_identity(chain TEXT PRIMARY KEY); INSERT INTO network_identity VALUES('eip155:56:usdt18');")?;
        }
        db.execute_batch("CREATE TABLE IF NOT EXISTS chain_receipts(tx_hash TEXT PRIMARY KEY, action_id TEXT NOT NULL UNIQUE, verified_at TEXT NOT NULL);")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS agent_public_exclusions(agent_id TEXT PRIMARY KEY,excluded_at TEXT NOT NULL);")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS registry_public_exclusions(registry_id TEXT PRIMARY KEY,excluded_at TEXT NOT NULL);")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS x402_quotes(id TEXT PRIMARY KEY,owner TEXT NOT NULL,agent_id TEXT NOT NULL,payload TEXT NOT NULL,amount TEXT NOT NULL,day INTEGER NOT NULL,expires INTEGER NOT NULL,status TEXT NOT NULL,signature TEXT,tx_hash TEXT UNIQUE); CREATE INDEX IF NOT EXISTS x402_agent_day ON x402_quotes(agent_id,day);")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS paid_deliveries(
          quote_id TEXT PRIMARY KEY,owner TEXT NOT NULL,agent_id TEXT NOT NULL,
          task_hash TEXT NOT NULL,provider TEXT NOT NULL,resource_url TEXT NOT NULL,
          tx_hash TEXT NOT NULL,response_hash TEXT NOT NULL,received_at TEXT NOT NULL,
          service_status TEXT NOT NULL CHECK(service_status IN ('completed','failed_after_payment','unavailable')),
          data TEXT,body_bytes INTEGER NOT NULL);
          CREATE INDEX IF NOT EXISTS paid_delivery_agent ON paid_deliveries(owner,agent_id,received_at);")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| ApiError::internal())?;
        }
        db.execute_batch("PRAGMA journal_mode=WAL;
          CREATE TABLE IF NOT EXISTS agent_plans(id TEXT PRIMARY KEY,owner TEXT NOT NULL,payload TEXT NOT NULL,created_at TEXT NOT NULL);
          CREATE INDEX IF NOT EXISTS agent_plan_owner ON agent_plans(owner);
          CREATE TABLE IF NOT EXISTS runtime_agents(id TEXT PRIMARY KEY,owner TEXT NOT NULL,payload TEXT NOT NULL,created_at TEXT NOT NULL);
          CREATE INDEX IF NOT EXISTS runtime_owner ON runtime_agents(owner);
          CREATE TABLE IF NOT EXISTS agent_events(id INTEGER PRIMARY KEY AUTOINCREMENT,agent_id TEXT NOT NULL,kind TEXT NOT NULL,status TEXT NOT NULL,at TEXT NOT NULL,message TEXT NOT NULL,provider TEXT,amount TEXT,currency TEXT,tx_hash TEXT,run_id TEXT);
          CREATE TABLE IF NOT EXISTS agent_runs(id TEXT PRIMARY KEY,agent_id TEXT NOT NULL,status TEXT NOT NULL,started_at TEXT NOT NULL,finished_at TEXT,output TEXT NOT NULL);
          CREATE UNIQUE INDEX IF NOT EXISTS active_agent_run ON agent_runs(agent_id) WHERE status='running';
          CREATE TABLE IF NOT EXISTS job_runs(id TEXT PRIMARY KEY,job_id TEXT NOT NULL,agent_id TEXT NOT NULL,owner TEXT NOT NULL,status TEXT NOT NULL,started_at TEXT NOT NULL,finished_at TEXT,output TEXT NOT NULL);
          CREATE UNIQUE INDEX IF NOT EXISTS active_job_run ON job_runs(job_id) WHERE status='running';
          CREATE UNIQUE INDEX IF NOT EXISTS active_job_agent_run ON job_runs(agent_id) WHERE status='running';
          CREATE INDEX IF NOT EXISTS job_run_history ON job_runs(job_id,started_at);
          CREATE TABLE IF NOT EXISTS wallet_challenges(agent_id TEXT PRIMARY KEY,wallet TEXT NOT NULL,message TEXT NOT NULL,expires_at TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS agent_keys(hash TEXT PRIMARY KEY,agent_id TEXT NOT NULL,created_at TEXT NOT NULL);
          CREATE UNIQUE INDEX IF NOT EXISTS unique_agent_registration ON runtime_agents(json_extract(payload,'$.registration_tx')) WHERE json_extract(payload,'$.registration_tx') IS NOT NULL;
          CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY,owner TEXT NOT NULL,payload TEXT NOT NULL,root_id TEXT NOT NULL,parent_id TEXT,created_at TEXT NOT NULL);
          CREATE INDEX IF NOT EXISTS jobs_root ON jobs(root_id);
          CREATE TABLE IF NOT EXISTS job_intents(id TEXT PRIMARY KEY,job_id TEXT NOT NULL,owner TEXT NOT NULL,payload TEXT NOT NULL,tx_hash TEXT UNIQUE,confirmed INTEGER NOT NULL DEFAULT 0,instruction TEXT);
          CREATE TABLE IF NOT EXISTS inference_usage(run_id TEXT PRIMARY KEY,agent_id TEXT NOT NULL,day TEXT NOT NULL,reserved INTEGER NOT NULL,cost INTEGER,status TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS bounties(id TEXT PRIMARY KEY,owner TEXT NOT NULL,payload TEXT NOT NULL,created_at TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS bond_reservations(job_id TEXT PRIMARY KEY,agent_id TEXT NOT NULL,token_address TEXT NOT NULL,amount TEXT NOT NULL,status TEXT NOT NULL,tx_hash TEXT);
          CREATE TABLE IF NOT EXISTS fee_receipts(id TEXT PRIMARY KEY,job_id TEXT NOT NULL,amount_micros INTEGER NOT NULL,destination TEXT NOT NULL,tx_hash TEXT NOT NULL UNIQUE,verified_at TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS buyback_receipts(id TEXT PRIMARY KEY,token_address TEXT NOT NULL,usdt_micros INTEGER NOT NULL,tokens TEXT NOT NULL,tx_hash TEXT NOT NULL UNIQUE,verified_at TEXT NOT NULL);")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS wallet_intents(id TEXT PRIMARY KEY,owner TEXT NOT NULL,agent_id TEXT NOT NULL,payload TEXT NOT NULL,instruction TEXT NOT NULL,tx_hash TEXT UNIQUE,confirmed INTEGER NOT NULL DEFAULT 0);")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS sponsor_challenges(agent_id TEXT PRIMARY KEY,request_id TEXT NOT NULL UNIQUE,owner TEXT NOT NULL,payload TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS sponsor_jobs(request_id TEXT PRIMARY KEY,agent_id TEXT NOT NULL,owner TEXT NOT NULL,wallet TEXT NOT NULL,sponsor TEXT NOT NULL,nonce INTEGER NOT NULL,owner_nonce INTEGER NOT NULL,created_at TEXT NOT NULL,status TEXT NOT NULL CHECK(status IN ('pending','confirmed','failed')),tx_hash TEXT NOT NULL UNIQUE,reserved_wei INTEGER NOT NULL,actual_wei INTEGER,settled_at TEXT,payload TEXT NOT NULL,UNIQUE(sponsor,nonce));
          CREATE UNIQUE INDEX IF NOT EXISTS sponsor_pending_agent ON sponsor_jobs(agent_id) WHERE status='pending';
          CREATE UNIQUE INDEX IF NOT EXISTS sponsor_pending_wallet ON sponsor_jobs(wallet) WHERE status='pending';")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS accounts(owner TEXT PRIMARY KEY,display_name TEXT NOT NULL,created_at TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS agent_creation_requests(owner TEXT NOT NULL,request_key TEXT NOT NULL,agent_id TEXT NOT NULL,plan_hash TEXT NOT NULL,PRIMARY KEY(owner,request_key));")?;
        db.execute_batch(
            "CREATE VIEW IF NOT EXISTS unlisted_public_agents AS
          SELECT agent_id FROM agent_public_exclusions
          UNION SELECT a.id AS agent_id FROM runtime_agents a JOIN registry_public_exclusions x
            ON lower(x.registry_id)=lower(json_extract(a.payload,'$.registry_id'));",
        )?;
        let event_columns = db
            .prepare("PRAGMA table_info(agent_events)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if !event_columns.iter().any(|column| column == "run_id") {
            db.execute("ALTER TABLE agent_events ADD COLUMN run_id TEXT", [])?;
        }
        db.execute(
            "CREATE INDEX IF NOT EXISTS agent_event_run ON agent_events(agent_id,run_id,id)",
            [],
        )?;
        let mut statement = db.prepare("PRAGMA table_info(job_intents)")?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if !columns.iter().any(|column| column == "instruction") {
            db.execute("ALTER TABLE job_intents ADD COLUMN instruction TEXT", [])?;
        }
        db.execute(
            "UPDATE agent_runs SET status='interrupted',finished_at=? WHERE status='running'",
            [now()],
        )?;
        db.execute(
            "UPDATE job_runs SET status='interrupted',finished_at=? WHERE status='running'",
            [now()],
        )?;
        Ok(store)
    }
    pub fn claim_receipt(&self, tx_hash: &str, action_id: &str) -> Result<()> {
        let db = self.connect()?;
        let existing: Option<String> = db
            .query_row(
                "SELECT action_id FROM chain_receipts WHERE tx_hash=?",
                [tx_hash.to_lowercase()],
                |r| r.get(0),
            )
            .optional()?;
        if existing.as_deref() == Some(action_id) {
            return Ok(());
        }
        if existing.is_some() {
            return Err(ApiError::conflict(
                "This transaction already confirms another action.",
            ));
        }
        db.execute(
            "INSERT INTO chain_receipts VALUES(?,?,?)",
            params![tx_hash.to_lowercase(), action_id, now()],
        )
        .map_err(|_| ApiError::conflict("This action already has a transaction receipt."))?;
        Ok(())
    }
    pub fn connect(&self) -> Result<Connection> {
        let db = Connection::open(&self.path)?;
        db.busy_timeout(Duration::from_secs(5))?;
        Ok(db)
    }
    pub fn publicly_excluded(db: &Connection, agent_id: &str) -> Result<bool> {
        Ok(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM unlisted_public_agents WHERE agent_id=?)",
            [agent_id],
            |r| r.get(0),
        )?)
    }
    pub fn excluded_registry_ids(db: &Connection) -> Result<std::collections::HashSet<String>> {
        let mut statement = db.prepare("SELECT lower(registry_id) FROM registry_public_exclusions UNION SELECT lower(json_extract(a.payload,'$.registry_id')) FROM runtime_agents a JOIN unlisted_public_agents e ON e.agent_id=a.id WHERE json_extract(a.payload,'$.registry_id') IS NOT NULL")?;
        let ids = statement.query_map([], |r| r.get::<_, String>(0))?;
        Ok(ids.collect::<std::result::Result<_, _>>()?)
    }
    pub fn agent(&self, owner: &str, id: &str) -> Result<RuntimeAgent> {
        Self::agent_in(&self.connect()?, owner, id)
    }
    pub fn agent_in(db: &Connection, owner: &str, id: &str) -> Result<RuntimeAgent> {
        let payload: Option<String> = db
            .query_row(
                "SELECT payload FROM runtime_agents WHERE owner=? AND id=?",
                params![owner, id],
                |r| r.get(0),
            )
            .optional()?;
        decode(payload.ok_or_else(|| ApiError::missing("Agent not found."))?)
    }
    pub fn agent_any(db: &Connection, id: &str) -> Result<(String, RuntimeAgent)> {
        let row: Option<(String, String)> = db
            .query_row(
                "SELECT owner,payload FROM runtime_agents WHERE id=?",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (owner, payload) = row.ok_or_else(|| ApiError::missing("Agent not found."))?;
        Ok((owner, decode(payload)?))
    }
    pub fn list_agents(&self, owner: &str) -> Result<Vec<RuntimeAgent>> {
        Self::list_payload(
            &self.connect()?,
            "SELECT payload FROM runtime_agents WHERE owner=? ORDER BY created_at DESC",
            [owner],
        )
    }
    pub fn list_plans(&self, owner: &str) -> Result<Vec<AgentPlan>> {
        Self::list_payload(
            &self.connect()?,
            "SELECT payload FROM agent_plans WHERE owner=? ORDER BY created_at DESC",
            [owner],
        )
    }
    pub fn list_payload<T: DeserializeOwned, P: rusqlite::Params>(
        db: &Connection,
        sql: &str,
        params: P,
    ) -> Result<Vec<T>> {
        let mut stmt = db.prepare(sql)?;
        let rows = stmt.query_map(params, |r| r.get::<_, String>(0))?;
        rows.map(|row| decode(row?)).collect()
    }
    pub fn save_agent(&self, agent: &RuntimeAgent) -> Result<()> {
        Self::save_agent_in(&self.connect()?, agent)
    }
    pub fn save_agent_in(db: &Connection, agent: &RuntimeAgent) -> Result<()> {
        db.execute(
            "UPDATE runtime_agents SET payload=? WHERE id=?",
            params![encode(agent)?, agent.id],
        )?;
        Ok(())
    }
    pub fn job(db: &Connection, id: &str) -> Result<Job> {
        let payload: Option<String> = db
            .query_row("SELECT payload FROM jobs WHERE id=?", [id], |r| r.get(0))
            .optional()?;
        decode(payload.ok_or_else(|| ApiError::missing("Job not found."))?)
    }
    pub fn save_job(db: &Connection, job: &Job) -> Result<()> {
        db.execute(
            "UPDATE jobs SET payload=? WHERE id=?",
            params![encode(job)?, job.id],
        )?;
        Ok(())
    }
    pub fn runs(&self, id: &str) -> Result<Vec<AgentRun>> {
        let db = self.connect()?;
        let mut stmt=db.prepare("SELECT id,agent_id,status,started_at,finished_at,output FROM agent_runs WHERE agent_id=? ORDER BY started_at DESC LIMIT 20")?;
        let rows = stmt.query_map([id], |r| {
            Ok(AgentRun {
                id: r.get(0)?,
                agent_id: r.get(1)?,
                status: r.get(2)?,
                started_at: r.get(3)?,
                finished_at: r.get(4)?,
                output: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
            })
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }
}
pub fn encode(value: &impl Serialize) -> Result<String> {
    serde_json::to_string(value).map_err(Into::into)
}
pub fn decode<T: DeserializeOwned>(value: String) -> Result<T> {
    serde_json::from_str(&value).map_err(Into::into)
}
