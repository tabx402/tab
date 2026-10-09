//! Account identity comes from verified Privy JWTs; wallets prove ownership separately.
use crate::{error::Result, models::*, AppState};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

impl AppState {
    pub fn account_profile(&self, owner: &str) -> Result<Value> {
        let profile: Option<(String, String)> = self
            .store
            .connect()?
            .query_row(
                "SELECT display_name,created_at FROM accounts WHERE owner=?",
                [owner],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let agents = self.store.list_agents(owner)?;
        Ok(
            json!({"registered":profile.is_some(),"display_name":profile.as_ref().map(|p|p.0.as_str()).unwrap_or(""),
            "created_at":profile.as_ref().map(|p|&p.1),"agents":agents.len(),
            "registered_agents":agents.iter().filter(|a|a.registry_id.is_some()).count(),
            "chain_id":56,"login_methods":["email","wallet"],"wallet_recovery":"privy",
            "model_credits":"platform funded, subject to daily cap"}),
        )
    }
    pub fn register_account(&self, owner: &str, input: AccountInput) -> Result<Value> {
        let name = if input.display_name.trim().is_empty() {
            String::new()
        } else {
            clean(&input.display_name, 2, 48)?
        };
        self.store.connect()?.execute("INSERT INTO accounts VALUES(?,?,?) ON CONFLICT(owner) DO UPDATE SET display_name=CASE WHEN excluded.display_name='' THEN accounts.display_name ELSE excluded.display_name END",params![owner,name,now()])?;
        self.account_profile(owner)
    }
}
