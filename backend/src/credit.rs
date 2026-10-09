//! Authenticated credit views derived from the verified custody contract.
use crate::{
    bnb,
    db::{decode, Store},
    error::{ApiError, Result},
    models::*,
    AppState,
};
use rusqlite::params;
use serde_json::{json, Value};
use std::collections::BTreeSet;
impl AppState {
    pub async fn secured_assets(&self) -> Result<Value> {
        if self.bnb.manifest()?["credit_mode"] != "collateralized" {
            let assets = self
                .backing_assets()
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|mut asset| {
                    asset["borrowing_enabled"] = json!(false);
                    asset["borrowing_status"] = json!("unsupported_legacy");
                    asset["price_usdt"] = Value::Null;
                    asset["risk"] = Value::Null;
                    asset
                })
                .collect::<Vec<_>>();
            return Ok(
                json!({"chain_id":56,"currency":"USDT","status":"unsupported_legacy","supported":false,"assets":assets,"stock_loans":self.finance_system().await.modules.stock_loans}),
            );
        }
        let live = self.bnb.deployed().await;
        let mut assets = vec![];
        for mut asset in self
            .backing_assets()
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            let Some(token) = asset["address"].as_str().map(str::to_owned) else {
                continue;
            };
            let risk = if live {
                self.bnb
                    .view("backing", "collateralAssets", vec![bnb::addr(&token)?])
                    .await
                    .ok()
            } else {
                None
            };
            let configured = risk
                .as_ref()
                .is_some_and(|r| bnb::number(&r["ltvBps"]).unwrap_or(0) > 0);
            let enabled = configured && risk.as_ref().is_some_and(|r| r["paused"] != true);
            let price = if enabled {
                self.bnb
                    .view("backing", "collateralPrice", vec![bnb::addr(&token)?])
                    .await
                    .ok()
                    .and_then(|v| bnb::number(&v).ok())
            } else {
                None
            };
            asset["borrowing_enabled"] = json!(enabled && price.is_some());
            asset["borrowing_status"] = json!(if !live {
                "contracts_unavailable"
            } else if risk.is_none() {
                "upgrade_required"
            } else if !configured {
                "awaiting_token_oracle"
            } else if !enabled {
                "borrowing_paused"
            } else if price.is_none() {
                "oracle_unavailable"
            } else {
                "ready"
            });
            asset["price_usdt"] = json!(price.map(|v| money(v).to_string()));
            asset["risk"] = json!(risk);
            assets.push(asset);
        }
        Ok(
            json!({"chain_id":56,"currency":"USDT","assets":assets,"interest_bps":0,"collateral_scope":"separate pledge per credit agreement"}),
        )
    }
    pub fn wallet_actions(&self, owner: &str, id: &str) -> Result<Vec<TransactionIntent>> {
        self.store.agent(owner, id)?;
        Store::list_payload(&self.store.connect()?,"SELECT json_set(payload,'$.tx_hash',tx_hash) FROM wallet_intents WHERE owner=? AND agent_id=? AND confirmed=0 AND (tx_hash IS NOT NULL OR julianday(json_extract(payload,'$.expires_at'))>julianday(?)) ORDER BY rowid DESC",params![owner,id,now()])
    }
    pub async fn credit_accounts(&self, owner: &str, id: &str) -> Result<Value> {
        let agent = self.store.agent(owner, id)?;
        let pending = self.wallet_actions(owner, id)?;
        if agent.registry_id.is_none() {
            return Ok(
                json!({"chain_id":56,"currency":"USDT","status":"not_registered","agreements":[],"pending_intents":pending}),
            );
        }
        self.bnb.require_deployment().await?;
        let wallet = self.verify_owned_agent(&agent).await?;
        let key = agent.registry_id.as_deref().expect("registered agent");
        let head = bnb::number(&self.bnb.rpc("eth_blockNumber", json!([])).await?)?;
        let final_block = head.saturating_sub(u128::from(self.config.confirmations - 1));
        let start = self.bnb.manifest()?["deployment_block"]
            .as_u64()
            .ok_or_else(|| ApiError::unavailable("Deployment start block is unavailable."))?;
        let mut ids = BTreeSet::new();
        let topic = bnb::hash(b"CreditOpened(bytes32,bytes32,address,address,uint256)");
        let lender_topic = format!("0x{:0>64}", wallet.trim_start_matches("0x"));
        for topics in [
            json!([topic, null, key]),
            json!([topic, null, null, lender_topic]),
        ] {
            let mut from = u128::from(start);
            while from <= final_block {
                let to = (from + 1999).min(final_block);
                let logs=self.bnb.rpc("eth_getLogs",json!([{"address":self.bnb.module("backing")?,"fromBlock":format!("0x{from:x}"),"toBlock":format!("0x{to:x}"),"topics":topics}])).await?;
                for log in logs
                    .as_array()
                    .ok_or_else(|| ApiError::unavailable("Credit history is unavailable."))?
                {
                    if log["removed"] == true {
                        continue;
                    }
                    if let Some(credit) = log["topics"][1].as_str() {
                        bnb::bytes32(credit)?;
                        ids.insert(credit.to_owned());
                    }
                }
                from = to + 1;
            }
        }
        let mut agreements = vec![];
        for credit in ids {
            let c = self
                .bnb
                .view("backing", "getCredit", vec![bnb::bytes32(&credit)?])
                .await?;
            let lender = c["lender"] == wallet;
            let borrower = c["borrower"] == wallet && c["agent"] == key;
            if !lender && !borrower {
                continue;
            }
            let expires = bnb::number(&c["expiresAt"])?;
            let expired = expires <= chrono::Utc::now().timestamp() as u128;
            let closed = c["closed"] == true;
            let accepted = c["accepted"] == true;
            let available = bnb::number(&c["available"])?;
            let outstanding = bnb::number(&c["outstanding"])?;
            let tools = bnb::number(&c["tools"])?;
            let linked:Option<String>=self.store.connect()?.query_row("SELECT payload FROM runtime_agents WHERE json_extract(payload,'$.registry_id')=?",[c["agent"].as_str().unwrap_or("")],|r|r.get(0)).ok();
            let local = linked
                .and_then(|v| decode::<RuntimeAgent>(v).ok())
                .filter(|a| a.wallet.as_deref() == Some(wallet.as_str()) || a.plan.public_activity)
                .map(|a| a.id);
            let mut row = json!({"id":credit,"agent_id":local,"onchain_agent_id":c["agent"],"lender":c["lender"],"borrower":c["borrower"],"signer":c["signer"],"role":if borrower{"borrower"}else{"lender"},"expires_at":expires,"tools":TOOLS.iter().enumerate().filter(|(i,_)|tools&(1<<i)!=0).map(|(_,t)|*t).collect::<Vec<_>>(),"recipients":c["recipients"],"accepted":accepted,"closed":closed,"status":if closed{"closed"}else if expired{"expired"}else if accepted{"active"}else{"awaiting_acceptance"},"interest_bps":0,"secured":false,"actions":{"accept":borrower&&!accepted&&!closed&&!expired,"repay":borrower&&outstanding>0,"withdraw":lender&&available>0,"close":!closed,"spend":borrower&&accepted&&!closed&&!expired&&available>0&&agent.status=="ready"}});
            if self.bnb.manifest()?["credit_mode"] == "collateralized" {
                let position = self
                    .bnb
                    .view(
                        "backing",
                        "collateralPositions",
                        vec![bnb::bytes32(&credit)?],
                    )
                    .await?;
                let collateral_token = position["token"].as_str().ok_or_else(ApiError::internal)?;
                let risk = self
                    .bnb
                    .view(
                        "backing",
                        "collateralAssets",
                        vec![bnb::addr(collateral_token)?],
                    )
                    .await?;
                let decimals = bnb::number(&risk["decimals"])? as u8;
                let pledged = bnb::number(&position["amount"])?;
                let valuation = self
                    .bnb
                    .view("backing", "collateralValue", vec![bnb::bytes32(&credit)?])
                    .await
                    .ok()
                    .and_then(|v| bnb::number(&v).ok());
                let power = valuation
                    .and_then(|v| v.checked_mul(bnb::number(&risk["ltvBps"]).unwrap_or(0)))
                    .map(|v| v / 10_000);
                let remaining_cap =
                    bnb::number(&risk["debtCap"])?.saturating_sub(bnb::number(&risk["debt"])?);
                let health = valuation.filter(|_| outstanding > 0).map(|v| {
                    rust_decimal::Decimal::from_i128_with_scale(v as i128, 18)
                        * rust_decimal::Decimal::from(
                            bnb::number(&risk["liquidationBps"]).unwrap_or(0) as u64,
                        )
                        / rust_decimal::Decimal::from(10_000u64)
                        / money(outstanding)
                });
                row["secured"] = json!(true);
                row["collateral"] = json!({"token":collateral_token,"decimals":decimals,"amount":rust_decimal::Decimal::from_i128_with_scale(pledged as i128,decimals.into()).normalize().to_string(),"value_usdt":valuation.map(|v|money(v).to_string()),"borrowing_power_usdt":power.map(|v|money(v).to_string()),"available_borrowing_usdt":power.map(|v|money(v.saturating_sub(outstanding).min(available).min(remaining_cap)).to_string()),"health_factor":health.map(|v|v.normalize().to_string()),"oracle_status":if valuation.is_some(){"verified"}else{"unavailable"},"ltv_bps":risk["ltvBps"],"liquidation_bps":risk["liquidationBps"],"bonus_bps":risk["bonusBps"],"paused":risk["paused"]});
                row["actions"]["pledge"] = json!(borrower);
                row["actions"]["withdraw_collateral"] = json!(
                    borrower
                        && pledged > 0
                        && (outstanding == 0 || power.is_some_and(|v| v > outstanding))
                );
                row["actions"]["spend"] = json!(
                    row["actions"]["spend"] == true
                        && risk["paused"] != true
                        && remaining_cap > 0
                        && power.is_some_and(|v| v > outstanding)
                );
                row["actions"]["liquidate"] = json!(
                    outstanding > 0
                        && valuation.is_some()
                        && (health.is_some_and(|v| v < rust_decimal::Decimal::ONE)
                            || chrono::Utc::now().timestamp() as u128 > expires + 86400)
                );
            } else {
                // Older agreements retain recovery access during an upgrade.
                row["actions"]["spend"] = json!(false);
                row["actions"]["accept"] = json!(false);
                row["security_status"] = json!("legacy_unsecured_recovery_only");
                for action in ["pledge", "withdraw_collateral", "liquidate"] {
                    row["actions"][action] = json!(false);
                }
            }
            for (field, key) in [
                ("funded", "funded"),
                ("available", "available"),
                ("outstanding", "outstanding"),
                ("total_spent", "totalSpent"),
                ("total_repaid", "totalRepaid"),
                ("withdrawn", "withdrawn"),
                ("per_call", "perCall"),
                ("daily_cap", "dailyCap"),
            ] {
                row[field] = json!(money(bnb::number(&c[key])?).to_string());
            }
            agreements.push(row);
        }
        Ok(
            json!({"chain_id":56,"currency":"USDT","status":"verified","agreements":agreements,"pending_intents":pending}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethabi::Token;

    const WALLET: &str = "0x4444444444444444444444444444444444444444";
    const BACKING: &str = "0x2222222222222222222222222222222222222222";
    const CREDIT: &str = "0x5555555555555555555555555555555555555555555555555555555555555555";

    fn call(signature: &str) -> String {
        format!("call:0x{}", &bnb::hash(signature.as_bytes())[2..10])
    }

    fn encoded(tokens: &[Token]) -> Value {
        json!(format!("0x{}", hex::encode(ethabi::encode(tokens))))
    }

    #[tokio::test]
    async fn verified_legacy_credit_reads_and_paused_expired_repayment_need_no_collateral_calls() {
        let f = crate::chain_tests::fixture().await;
        let mut agent = f.state.create_agent("owner", serde_json::from_value(json!({"name":"willow","purpose":"monitor a public wallet","daily_cap":"10","max_call":"1","tools":["bnb-rpc"],"public_activity":false})).unwrap()).unwrap();
        agent.wallet = Some(WALLET.into());
        agent.registry_id = Some(f.state.bnb.agent_address(WALLET, &agent.id).unwrap());
        agent.status = "paused".into();
        f.state.store.save_agent(&agent).unwrap();
        let key = agent.registry_id.as_deref().unwrap();
        let expires = (chrono::Utc::now().timestamp() - 86400) as u128;
        {
            let mut replies = f.replies.lock().unwrap();
            replies.insert(
                call("getAgent(bytes32)"),
                encoded(&[Token::Tuple(vec![
                    bnb::addr(WALLET).unwrap(),
                    Token::String("willow".into()),
                    bnb::uint(10_000_000_000_000_000_000),
                    Token::FixedBytes(vec![1; 32]),
                    Token::Bool(true),
                    bnb::uint(1),
                    bnb::uint(0),
                    bnb::uint(0),
                ])]),
            );
            replies.insert("eth_getLogs".into(), json!([{"address":BACKING,"topics":[bnb::hash(b"CreditOpened(bytes32,bytes32,address,address,uint256)"),CREDIT,key,format!("0x{:0>64}",BACKING.trim_start_matches("0x"))],"removed":false}]));
            replies.insert(
                call("getCredit(bytes32)"),
                encoded(&[Token::Tuple(vec![
                    bnb::bytes32(key).unwrap(),
                    bnb::addr(BACKING).unwrap(),
                    bnb::addr(WALLET).unwrap(),
                    bnb::addr(BACKING).unwrap(),
                    bnb::uint(10_000_000_000_000_000_000),
                    bnb::uint(7_000_000_000_000_000_000),
                    bnb::uint(3_000_000_000_000_000_000),
                    bnb::uint(3_000_000_000_000_000_000),
                    bnb::uint(0),
                    bnb::uint(0),
                    bnb::uint(1_000_000_000_000_000_000),
                    bnb::uint(3_000_000_000_000_000_000),
                    bnb::uint(0),
                    bnb::uint(0),
                    bnb::uint(expires),
                    bnb::uint(1),
                    Token::Bool(true),
                    Token::Bool(false),
                    Token::Array(vec![bnb::addr(BACKING).unwrap()]),
                ])]),
            );
        }

        let accounts = f.state.credit_accounts("owner", &agent.id).await.unwrap();
        assert_eq!(accounts["status"], "verified");
        let agreements = accounts["agreements"].as_array().unwrap();
        assert_eq!(agreements.len(), 1);
        let agreement = &agreements[0];
        assert_eq!(agreement["id"], CREDIT);
        assert_eq!(agreement["status"], "expired");
        assert_eq!(agreement["secured"], false);
        assert_eq!(agreement["outstanding"], "3");
        assert_eq!(agreement["available"], "7");
        assert_eq!(agreement["actions"]["repay"], true);
        assert_eq!(agreement["actions"]["spend"], false);
        for action in ["pledge", "withdraw_collateral", "liquidate"] {
            assert_eq!(agreement["actions"][action], false);
        }
        assert!(agreement["collateral"].is_null());

        let input = serde_json::from_value(
            json!({"action":"repay_credit","amount":"1","credit_id":CREDIT}),
        )
        .unwrap();
        let intent = f
            .state
            .financial_prepare("owner", &agent.id, input)
            .await
            .unwrap();
        assert_eq!(intent.to, BACKING);
        assert_eq!(intent.transactions.len(), 2);
        let abi = ethabi::Contract::load(
            include_bytes!("../../contracts/bnb/abi/TabBacking.json").as_slice(),
        )
        .unwrap();
        let calldata = hex::decode(intent.data.trim_start_matches("0x")).unwrap();
        let repay = abi.function("repayCredit").unwrap();
        assert_eq!(&calldata[..4], &repay.short_signature());
        assert_eq!(
            repay.decode_input(&calldata[4..]).unwrap(),
            vec![
                bnb::bytes32(CREDIT).unwrap(),
                bnb::uint(1_000_000_000_000_000_000)
            ]
        );
        let replies = f.replies.lock().unwrap();
        for method in [
            "collateralPositions(bytes32)",
            "collateralAssets(address)",
            "collateralValue(bytes32)",
            "collateralPrice(address)",
        ] {
            assert!(!replies.contains_key(&format!("request:{}", call(method))));
        }
    }
}
