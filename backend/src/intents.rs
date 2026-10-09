//! Retain submitted wallet transactions before finality, including across browser restarts.
use crate::{
    bnb::{self, Instruction},
    db::decode,
    error::{ApiError, Result},
    AppState,
};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
/// Serialize reviewed transactions across panels for the same authenticated wallet.
/// An uncertain submission retains its reservation even after preparation expiry.
pub(crate) fn has_pending_transaction(
    db: &rusqlite::Connection,
    owner: &str,
    sender: &str,
) -> Result<bool> {
    for table in ["wallet_intents", "job_intents"] {
        let sql=format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE owner=? AND lower(json_extract(payload,'$.sender'))=lower(?) AND confirmed=0 AND (tx_hash IS NOT NULL OR julianday(json_extract(payload,'$.expires_at'))>julianday(?)))");
        if db.query_row(&sql, params![owner, sender, crate::models::now()], |r| {
            r.get::<_, bool>(0)
        })? {
            return Ok(true);
        }
    }
    Ok(false)
}
impl AppState {
    pub async fn submitted_action(
        &self,
        owner: &str,
        id: &str,
        hash: &str,
        job: bool,
    ) -> Result<Value> {
        bnb::signature(hash)?;
        let hash = hash.to_lowercase();
        let table = if job { "job_intents" } else { "wallet_intents" };
        let sql = format!(
            "SELECT payload,instruction,tx_hash,confirmed FROM {table} WHERE owner=? AND id=?"
        );
        let row: Option<(String, String, Option<String>, u8)> = self
            .store
            .connect()?
            .query_row(&sql, params![owner, id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .optional()?;
        let (payload, instruction, prior, confirmed) =
            row.ok_or_else(|| ApiError::missing("Wallet action not found."))?;
        if confirmed == 2 || prior.as_ref().is_some_and(|v| v != &hash) {
            return Err(ApiError::conflict(
                "This action is bound to its original submitted transaction.",
            ));
        }
        let payload: Value = serde_json::from_str(&payload)?;
        let ix: Instruction = decode(instruction)?;
        let sender = payload["sender"].as_str().ok_or_else(ApiError::internal)?;
        self.bnb.verify_envelope(&hash, sender, &ix).await?;
        let sql=format!("UPDATE {table} SET tx_hash=? WHERE owner=? AND id=? AND (tx_hash IS NULL OR tx_hash=?) AND confirmed IN (0,1)");
        let rows = self
            .store
            .connect()?
            .execute(&sql, params![hash, owner, id, hash])
            .map_err(|_| ApiError::conflict("Transaction already belongs to another action."))?;
        if rows != 1 {
            return Err(ApiError::conflict(
                "Action changed while checking the submitted transaction.",
            ));
        }
        Ok(
            json!({"status":if confirmed==1{"confirmed"}else{"submitted"},"tx_hash":hash,"intent_id":id}),
        )
    }
    pub async fn release_failed_action(&self, owner: &str, id: &str, job: bool) -> Result<Value> {
        let table = if job { "job_intents" } else { "wallet_intents" };
        let sql = format!("SELECT tx_hash FROM {table} WHERE owner=? AND id=? AND confirmed=0");
        let hash: Option<Option<String>> = self
            .store
            .connect()?
            .query_row(&sql, params![owner, id], |r| r.get(0))
            .optional()?;
        let hash = hash
            .flatten()
            .ok_or_else(|| ApiError::missing("Submitted pending action not found."))?;
        if self.bnb.transaction_status(&hash).await?["status"] != "failed" {
            return Err(ApiError::conflict(
                "Only a final, reverted transaction can release this submitted action.",
            ));
        }
        self.store.connect()?.execute(&format!("UPDATE {table} SET confirmed=2 WHERE owner=? AND id=? AND tx_hash=? AND confirmed=0"),params![owner,id,hash])?;
        Ok(json!({"status":"released","tx_hash":hash,"intent_id":id}))
    }
}
