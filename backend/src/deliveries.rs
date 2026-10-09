//! Owner-private merchant outputs survive payment reconciliation and page reloads.
//! A delivered body is usable only after its exact payment receipt is confirmed.
use crate::{
    bnb,
    db::{decode, encode},
    error::{ApiError, Result},
    models::{money, now},
    AppState,
};
use chrono::{Duration, Utc};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};

const MAX_DELIVERY_BYTES: usize = 65_536;
const UNAVAILABLE_BODY: &[u8] =
    b"Service response was unavailable; the payment receipt is retained for reconciliation.";

impl AppState {
    /// Store the response before waiting for the canonical receipt. Never repeat
    /// a payment or replace the first observed body to recover a missing result.
    pub(crate) fn capture_paid_delivery(
        &self,
        owner: &str,
        agent_id: &str,
        quote_id: &str,
        tx_hash: &str,
        body: &[u8],
        service_ok: bool,
    ) -> Result<()> {
        bnb::signature(tx_hash)?;
        let tx_hash = tx_hash.to_lowercase();
        let response_hash = bnb::hash(body);
        let available =
            !body.is_empty() && body.len() <= MAX_DELIVERY_BYTES && body != UNAVAILABLE_BODY;
        let data = if available {
            serde_json::from_slice::<Value>(body)
                .ok()
                .or_else(|| {
                    std::str::from_utf8(body)
                        .ok()
                        .map(|text| Value::String(text.into()))
                })
                .filter(|value| !value.is_null())
        } else {
            None
        };
        let service_status = if data.is_none() {
            "unavailable"
        } else if service_ok {
            "completed"
        } else {
            "failed_after_payment"
        };
        let mut db = self.store.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(String, String, Option<String>)> = tx
            .query_row(
                "SELECT payload,status,tx_hash FROM x402_quotes WHERE id=? AND owner=? AND agent_id=?",
                params![quote_id, owner, agent_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (payload, status, prior_hash) =
            row.ok_or_else(|| ApiError::missing("Paid request not found."))?;
        if !["submitted", "uncertain", "confirmed"].contains(&status.as_str()) {
            return Err(ApiError::conflict("This paid request was not submitted."));
        }
        if prior_hash
            .as_deref()
            .is_some_and(|hash| !hash.eq_ignore_ascii_case(&tx_hash))
        {
            return Err(ApiError::conflict(
                "This paid request already has a different payment receipt.",
            ));
        }
        let quote: Value = decode(payload)?;
        let prior: Option<(String, String)> = tx
            .query_row(
                "SELECT tx_hash,response_hash FROM paid_deliveries WHERE quote_id=? AND owner=? AND agent_id=?",
                params![quote_id, owner, agent_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((payment, response)) = prior {
            if payment != tx_hash || response != response_hash {
                return Err(ApiError::conflict(
                    "The first provider response is already retained for this payment.",
                ));
            }
            return Ok(());
        }
        tx.execute(
            "INSERT INTO paid_deliveries VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                quote_id,
                owner,
                agent_id,
                quote["task_hash"].as_str().unwrap_or(""),
                quote["merchant"]["id"]
                    .as_str()
                    .ok_or_else(ApiError::internal)?,
                quote["merchant"]["resource_url"]
                    .as_str()
                    .ok_or_else(ApiError::internal)?,
                tx_hash,
                response_hash,
                now(),
                service_status,
                data.as_ref().map(encode).transpose()?,
                body.len() as i64,
            ],
        )?;
        tx.execute(
            "UPDATE x402_quotes SET tx_hash=? WHERE id=? AND owner=? AND agent_id=?",
            params![tx_hash, quote_id, owner, agent_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// History remains private even when an agent publishes its run activity.
    /// Payment status and provider delivery status are separate facts.
    pub(crate) fn paid_delivery_history(
        &self,
        owner: &str,
        agent_id: &str,
        quote_id: &str,
    ) -> Result<Value> {
        self.store.agent(owner, agent_id)?;
        let db = self.store.connect()?;
        let quote: Option<(String, String, Option<String>, String)> = db
            .query_row(
                "SELECT payload,status,tx_hash,amount FROM x402_quotes WHERE id=? AND owner=? AND agent_id=?",
                params![quote_id, owner, agent_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let (payload, payment_status, payment_hash, amount) =
            quote.ok_or_else(|| ApiError::missing("Paid request not found."))?;
        let quote: Value = decode(payload)?;
        let amount = amount.parse::<u128>().map_err(|_| ApiError::internal())?;
        let delivery: Option<(String, String, String, String, Option<String>, i64)> = db
            .query_row(
                "SELECT tx_hash,response_hash,received_at,service_status,data,body_bytes FROM paid_deliveries WHERE quote_id=? AND owner=? AND agent_id=?",
                params![quote_id, owner, agent_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .optional()?;
        let canonical: bool = if let Some(hash) = &payment_hash {
            db.query_row(
                "SELECT EXISTS(SELECT 1 FROM chain_receipts WHERE tx_hash=? AND action_id=?)",
                params![hash.to_lowercase(), quote_id],
                |r| r.get(0),
            )?
        } else {
            false
        };
        let payment_confirmed = payment_status == "confirmed" && canonical;
        let mut value = json!({
            "status":if payment_confirmed {"unavailable"}else{"pending_payment"},
            "quote_id":quote_id,"provider":quote["merchant"]["id"],
            "resource_url":quote["merchant"]["resource_url"],
            "received_at":null,"response_hash":null,"tx_hash":payment_hash,
            "amount":money(amount).to_string(),"currency":"USDT",
            "settled_usdt":payment_confirmed,"data":null,"cached":true,
        });
        if let Some((hash, response_hash, received_at, service_status, data, body_bytes)) = delivery
        {
            let settled = payment_confirmed
                && payment_hash
                    .as_deref()
                    .is_some_and(|p| p.eq_ignore_ascii_case(&hash));
            value["status"] = json!(if settled {
                service_status.as_str()
            } else {
                "pending_payment"
            });
            value["settled_usdt"] = json!(settled);
            value["received_at"] = json!(received_at);
            // An unavailable body may be a transport diagnostic rather than
            // merchant bytes. It cannot serve as a delivered-content proof.
            value["response_hash"] = if data.is_some() {
                json!(response_hash)
            } else {
                Value::Null
            };
            value["body_bytes"] = if data.is_some() {
                json!(body_bytes)
            } else {
                Value::Null
            };
            value["data"] = data
                .map(decode::<Value>)
                .transpose()?
                .unwrap_or(Value::Null);
        }
        Ok(value)
    }

    /// Reuse genuine paid data without issuing another payment. Automatic reuse
    /// expires after 15 minutes; explicit selection retains the observed time.
    pub(crate) fn paid_delivery_for_run(
        &self,
        owner: &str,
        agent_id: &str,
        quote_id: Option<&str>,
    ) -> Result<Option<Value>> {
        let agent = self.store.agent(owner, agent_id)?;
        let policy = self.policy(&agent);
        let db = self.store.connect()?;
        let cutoff = (Utc::now() - Duration::minutes(15)).to_rfc3339();
        let future = (Utc::now() + Duration::seconds(30)).to_rfc3339();
        let selected: Option<String> = db
            .query_row(
                "SELECT d.quote_id FROM paid_deliveries d
             JOIN x402_quotes q ON q.id=d.quote_id AND q.owner=d.owner AND q.agent_id=d.agent_id
             JOIN chain_receipts r ON r.tx_hash=d.tx_hash AND r.action_id=d.quote_id
             WHERE d.owner=? AND d.agent_id=? AND d.task_hash=?
               AND json_extract(q.payload,'$.task_hash')=d.task_hash
               AND q.status='confirmed' AND lower(q.tx_hash)=d.tx_hash
               AND d.service_status='completed' AND d.data IS NOT NULL
               AND julianday(d.received_at)<=julianday(?)
               AND (? IS NULL OR d.quote_id=?)
               AND (? IS NOT NULL OR julianday(d.received_at)>=julianday(?))
             ORDER BY julianday(d.received_at) DESC,d.rowid DESC LIMIT 1",
                params![owner, agent_id, policy, future, quote_id, quote_id, quote_id, cutoff],
                |r| r.get(0),
            )
            .optional()?;
        selected
            .map(|id| -> Result<Value> {
                let mut delivery = self.paid_delivery_history(owner, agent_id, &id)?;
                delivery["charged_in_run"] = json!(false);
                Ok(delivery)
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const HASH: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";

    fn setup() -> (tempfile::TempDir, AppState, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let config = crate::config::Config {
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
            database: root.join("data.sqlite"),
            manifest: root.join("manifest.json"),
            merchants: root.join("merchants.json"),
            x402_merchants: root.join("x402.json"),
            bind: "127.0.0.1:0".into(),
            openrouter_key: None,
            tavily_key: None,
            inference_daily_micros: 0,
        };
        let state = AppState::new(config).unwrap();
        let plan = serde_json::from_value(json!({
            "name":"willow","purpose":"read actual market data",
            "daily_cap":"1","max_call":"0.1","tools":["x402"],
        }))
        .unwrap();
        let agent = state.create_agent("owner", plan).unwrap();
        (dir, state, agent.id)
    }

    fn request(state: &AppState, agent_id: &str, quote_id: &str) {
        let agent = state.store.agent("owner", agent_id).unwrap();
        let payload = json!({
            "task_hash":state.policy(&agent),
            "merchant":{"id":"market-data","resource_url":"https://market.example/quote"},
        });
        state.store.connect().unwrap().execute(
            "INSERT INTO x402_quotes(id,owner,agent_id,payload,amount,day,expires,status,signature) VALUES(?,?,?,?,?,?,?,'submitted',?)",
            params![quote_id, "owner", agent_id, encode(&payload).unwrap(), "1000000000000", 0, 1000, "private authorization"],
        ).unwrap();
    }

    fn settle(state: &AppState, quote_id: &str) {
        let db = state.store.connect().unwrap();
        db.execute(
            "UPDATE x402_quotes SET status='confirmed',tx_hash=? WHERE id=?",
            params![HASH, quote_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO chain_receipts VALUES(?,?,?)",
            params![HASH, quote_id, now()],
        )
        .unwrap();
    }

    #[test]
    fn merchant_output_survives_reload_and_stays_private() {
        let (_dir, state, agent_id) = setup();
        request(&state, &agent_id, "purchase");
        state
            .capture_paid_delivery(
                "owner",
                &agent_id,
                "purchase",
                HASH,
                br#"{"price":"1.01","private_note":"owner-only"}"#,
                true,
            )
            .unwrap();
        settle(&state, "purchase");
        let reopened = AppState::new((*state.config).clone()).unwrap();
        let delivery = reopened
            .paid_delivery_history("owner", &agent_id, "purchase")
            .unwrap();
        assert_eq!(delivery["status"], "completed");
        assert_eq!(delivery["data"]["private_note"], "owner-only");
        assert_eq!(delivery["amount"], "0.000001");
        assert_eq!(delivery["settled_usdt"], true);
        assert!(!delivery.to_string().contains("private authorization"));
        assert!(!delivery.to_string().contains("signature"));
        assert!(reopened
            .paid_delivery_history("intruder", &agent_id, "purchase")
            .is_err());
        let second = reopened.create_agent("owner", serde_json::from_value(json!({"name":"other","purpose":"another actual task","daily_cap":"1","max_call":"0.1","tools":["x402"]})).unwrap()).unwrap();
        assert!(reopened
            .paid_delivery_history("owner", &second.id, "purchase")
            .is_err());
        assert!(reopened
            .paid_delivery_for_run("owner", &second.id, Some("purchase"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn response_waits_for_its_exact_canonical_payment() {
        let (_dir, state, agent_id) = setup();
        request(&state, &agent_id, "purchase");
        state
            .capture_paid_delivery(
                "owner",
                &agent_id,
                "purchase",
                HASH,
                br#"{"price":"1.01"}"#,
                true,
            )
            .unwrap();
        assert_eq!(
            state
                .paid_delivery_history("owner", &agent_id, "purchase")
                .unwrap()["status"],
            "pending_payment"
        );
        assert!(state
            .paid_delivery_for_run("owner", &agent_id, Some("purchase"))
            .unwrap()
            .is_none());
        let db = state.store.connect().unwrap();
        db.execute(
            "UPDATE x402_quotes SET status='confirmed' WHERE id='purchase'",
            [],
        )
        .unwrap();
        assert!(state
            .paid_delivery_for_run("owner", &agent_id, None)
            .unwrap()
            .is_none());
        db.execute(
            "INSERT INTO chain_receipts VALUES(?,'another-action',?)",
            params![HASH, now()],
        )
        .unwrap();
        assert_eq!(
            state
                .paid_delivery_history("owner", &agent_id, "purchase")
                .unwrap()["settled_usdt"],
            false
        );
        db.execute(
            "UPDATE chain_receipts SET action_id='purchase' WHERE tx_hash=?",
            [HASH],
        )
        .unwrap();
        assert!(state
            .paid_delivery_for_run("owner", &agent_id, None)
            .unwrap()
            .is_some());
        db.execute("UPDATE x402_quotes SET tx_hash='0x2222222222222222222222222222222222222222222222222222222222222222' WHERE id='purchase'", []).unwrap();
        assert!(state
            .paid_delivery_for_run("owner", &agent_id, Some("purchase"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn paid_provider_failure_is_retained_without_becoming_task_input() {
        let (_dir, state, agent_id) = setup();
        request(&state, &agent_id, "purchase");
        state
            .capture_paid_delivery(
                "owner",
                &agent_id,
                "purchase",
                HASH,
                br#"{"error":"provider unavailable"}"#,
                false,
            )
            .unwrap();
        settle(&state, "purchase");
        let delivery = state
            .paid_delivery_history("owner", &agent_id, "purchase")
            .unwrap();
        assert_eq!(delivery["status"], "failed_after_payment");
        assert_eq!(delivery["settled_usdt"], true);
        assert_eq!(delivery["data"]["error"], "provider unavailable");
        assert!(state
            .paid_delivery_for_run("owner", &agent_id, Some("purchase"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn unavailable_bodies_retain_payment_without_claiming_a_delivered_content_proof() {
        for body in [
            vec![b'x'; MAX_DELIVERY_BYTES + 1],
            vec![0xff, 0xfe],
            Vec::new(),
            b"null".to_vec(),
            UNAVAILABLE_BODY.to_vec(),
            UNAVAILABLE_BODY.to_vec(),
        ] {
            let (_dir, state, agent_id) = setup();
            request(&state, &agent_id, "purchase");
            state
                .capture_paid_delivery("owner", &agent_id, "purchase", HASH, &body, true)
                .unwrap();
            settle(&state, "purchase");
            let delivery = state
                .paid_delivery_history("owner", &agent_id, "purchase")
                .unwrap();
            assert_eq!(delivery["status"], "unavailable");
            assert!(delivery["response_hash"].is_null());
            assert!(delivery["body_bytes"].is_null());
            assert_eq!(delivery["tx_hash"], HASH);
            assert!(delivery["data"].is_null());
            assert!(state
                .paid_delivery_for_run("owner", &agent_id, None)
                .unwrap()
                .is_none());
        }
    }

    #[test]
    fn changed_tasks_and_legacy_quotes_cannot_supply_another_task() {
        let (_dir, state, agent_id) = setup();
        request(&state, &agent_id, "purchase");
        state
            .capture_paid_delivery(
                "owner",
                &agent_id,
                "purchase",
                HASH,
                b"market observation",
                true,
            )
            .unwrap();
        settle(&state, "purchase");
        assert!(state
            .paid_delivery_for_run("owner", &agent_id, None)
            .unwrap()
            .is_some());
        let mut agent = state.store.agent("owner", &agent_id).unwrap();
        agent.plan.purpose = "different new task".into();
        state.store.save_agent(&agent).unwrap();
        assert!(state
            .paid_delivery_for_run("owner", &agent_id, Some("purchase"))
            .unwrap()
            .is_none());
        assert_eq!(
            state
                .paid_delivery_history("owner", &agent_id, "purchase")
                .unwrap()["data"],
            "market observation"
        );
        let db = state.store.connect().unwrap();
        db.execute(
            "UPDATE paid_deliveries SET task_hash=? WHERE quote_id='purchase'",
            [state.policy(&agent)],
        )
        .unwrap();
        assert!(state
            .paid_delivery_for_run("owner", &agent_id, None)
            .unwrap()
            .is_none());
        db.execute(
            "UPDATE x402_quotes SET payload=json_remove(payload,'$.task_hash') WHERE id='purchase'",
            [],
        )
        .unwrap();
        assert!(state
            .paid_delivery_for_run("owner", &agent_id, Some("purchase"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn older_results_require_explicit_selection_and_keep_the_observed_time() {
        let (_dir, state, agent_id) = setup();
        request(&state, &agent_id, "purchase");
        state
            .capture_paid_delivery(
                "owner",
                &agent_id,
                "purchase",
                HASH,
                br#"{"observed_price":"1"}"#,
                true,
            )
            .unwrap();
        settle(&state, "purchase");
        let old = (Utc::now() - Duration::hours(2)).to_rfc3339();
        state
            .store
            .connect()
            .unwrap()
            .execute(
                "UPDATE paid_deliveries SET received_at=? WHERE quote_id='purchase'",
                [&old],
            )
            .unwrap();
        assert!(state
            .paid_delivery_for_run("owner", &agent_id, None)
            .unwrap()
            .is_none());
        let explicit = state
            .paid_delivery_for_run("owner", &agent_id, Some("purchase"))
            .unwrap()
            .unwrap();
        assert_eq!(explicit["received_at"], old);
        assert_eq!(explicit["cached"], true);
    }

    #[test]
    fn first_delivered_response_cannot_be_replaced_or_retimed() {
        let (_dir, state, agent_id) = setup();
        request(&state, &agent_id, "purchase");
        state
            .capture_paid_delivery(
                "owner",
                &agent_id,
                "purchase",
                HASH,
                b"first actual result",
                true,
            )
            .unwrap();
        let first = state
            .paid_delivery_history("owner", &agent_id, "purchase")
            .unwrap();
        state
            .capture_paid_delivery(
                "owner",
                &agent_id,
                "purchase",
                HASH,
                b"first actual result",
                true,
            )
            .unwrap();
        assert!(state
            .capture_paid_delivery(
                "owner",
                &agent_id,
                "purchase",
                HASH,
                b"replacement result",
                true
            )
            .is_err());
        assert_eq!(
            state
                .paid_delivery_history("owner", &agent_id, "purchase")
                .unwrap(),
            first
        );
    }
}
