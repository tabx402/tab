//! Merchant invoices authorize an exact provider payment, not a claim that work succeeded.
use crate::{
    bnb,
    db::{encode, Store},
    error::{ApiError, Result},
    models::*,
    AppState,
};
use chrono::Utc;
use rusqlite::params;
use serde_json::json;
pub fn invoice_digest(
    program: &str,
    job: &Job,
    merchant: &JobMerchant,
    invoice: &ServiceInvoice,
) -> Result<Vec<u8>> {
    let domain = ethabi::encode(&[
        bnb::bytes32(&bnb::hash(
            b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
        ))?,
        bnb::bytes32(&bnb::hash(b"TabJobInvoice"))?,
        bnb::bytes32(&bnb::hash(b"1"))?,
        bnb::uint(56),
        bnb::addr(program)?,
    ]);
    let body=ethabi::encode(&[bnb::bytes32(&bnb::hash(b"Invoice(bytes32 job,bytes32 termsHash,bytes32 serviceKey,address recipient,uint256 amount,uint64 tool,bytes32 requestHash,uint64 expires,bytes32 nonce)"))?,bnb::bytes32(&job.id)?,bnb::bytes32(&job.terms_hash)?,bnb::bytes32(&bnb::hash(merchant.id.as_bytes()))?,bnb::addr(&merchant.recipient)?,bnb::uint(units(invoice.amount)?),bnb::uint(tool_bitmap(&[merchant.tool.clone()]).into()),bnb::bytes32(&invoice.request_hash)?,bnb::uint(invoice.expires.into()),bnb::bytes32(&invoice.nonce)?]);
    let mut payload = vec![0x19, 0x01];
    payload.extend(
        hex::decode(bnb::hash(&domain).trim_start_matches("0x"))
            .map_err(|_| ApiError::internal())?,
    );
    payload.extend(
        hex::decode(bnb::hash(&body).trim_start_matches("0x")).map_err(|_| ApiError::internal())?,
    );
    hex::decode(bnb::hash(&payload).trim_start_matches("0x")).map_err(|_| ApiError::internal())
}
impl AppState {
    pub async fn prepare_invoice(
        &self,
        owner: &str,
        id: &str,
        invoice: ServiceInvoice,
    ) -> Result<JobIntent> {
        self.bnb.require_deployment().await?;
        let job = self.get_job(owner, id)?;
        let agent = self.store.agent(owner, &job.plan.executor_id)?;
        let wallet = self.verify_owned_agent(&agent).await?;
        if job.executor_wallet != wallet
            || agent.status != "ready"
            || job.state != "open"
            || job.funding != "funded"
            || job.paused
            || job.plan.deadline <= Utc::now()
        {
            return Err(ApiError::conflict(
                "Only the assigned agent can pay a service from this open, active job.",
            ));
        }
        if invoice.amount <= rust_decimal::Decimal::ZERO
            || invoice.amount > job.plan.max_call
            || invoice.amount > job.available
            || invoice.expires <= Utc::now().timestamp() as u64
            || invoice.expires > Utc::now().timestamp() as u64 + 600
        {
            return Err(ApiError::validation(
                "The invoice must fit the available job budget and expire within ten minutes.",
            ));
        }
        let merchant = job
            .root_services
            .iter()
            .find(|m| {
                m.id == invoice.service_id
                    && job.plan.services.contains(&m.id)
                    && job.plan.tools.contains(&m.tool)
            })
            .ok_or_else(|| {
                ApiError::forbidden("The buyer did not approve this provider for this branch.")
            })?;
        if bnb::id(&invoice.request_hash)? == bnb::ZERO_HASH
            || bnb::id(&invoice.nonce)? == bnb::ZERO_HASH
        {
            return Err(ApiError::validation(
                "Invoice request and nonce cannot be zero.",
            ));
        }
        let digest = invoice_digest(&self.config.program, &job, merchant, &invoice)?;
        if bnb::digest_signer(&digest, &invoice.signature)? != merchant.service_key {
            return Err(ApiError::forbidden(
                "The merchant did not sign these exact job payment terms.",
            ));
        }
        let commitment = bnb::hash(&serde_json::to_vec(&invoice)?);
        let ix = self.bnb.instruction(
            "protocol",
            "payCall",
            vec![
                bnb::bytes32(id)?,
                bnb::addr(&merchant.recipient)?,
                bnb::uint(units(invoice.amount)?),
                bnb::uint(tool_bitmap(&[merchant.tool.clone()]).into()),
                bnb::bytes32(&invoice.request_hash)?,
                bnb::bytes32(&commitment)?,
            ],
        )?;
        let transactions = self.bnb.transactions(&wallet, &ix, None).await?;
        let intent = JobIntent {
            tx_hash: None,
            id: identifier(),
            job_id: id.into(),
            action: "pay".into(),
            chain_id: 56,
            network: "mainnet".into(),
            sender: wallet,
            to: ix.to.clone(),
            data: ix.data.clone(),
            value: ix.value.clone(),
            approval: Some(
                json!({"invoice":invoice,"recipient":merchant.recipient,"receipt_commitment":commitment,"verification":"merchant-signed invoice"}),
            ),
            transactions,
            expires_at: chrono::DateTime::from_timestamp(invoice.expires as i64, 0)
                .ok_or_else(|| ApiError::validation("Invalid invoice expiry."))?
                .to_rfc3339(),
        };
        let mut db = self.store.connect()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let pending:Vec<JobIntent>=Store::list_payload(&tx,"SELECT payload FROM job_intents WHERE job_id=? AND confirmed=0 AND (tx_hash IS NOT NULL OR julianday(json_extract(payload,'$.expires_at'))>julianday(?))",params![id,now()])?;
        if let Some(prior) = pending.first() {
            if prior.action == "pay" && prior.data == intent.data {
                return Ok(prior.clone());
            }
            return Err(ApiError::conflict("Confirm the existing job action first."));
        }
        if encode(&Store::job(&tx, id)?)? != encode(&job)? {
            return Err(ApiError::conflict(
                "Job changed while verifying the invoice.",
            ));
        }
        tx.execute(
            "INSERT INTO job_intents(id,job_id,owner,payload,instruction) VALUES(?,?,?,?,?)",
            params![intent.id, id, owner, encode(&intent)?, encode(&ix)?],
        )?;
        tx.commit()?;
        Ok(intent)
    }
    pub fn invoice_payment_event(&self, owner: &str, intent: &JobIntent, hash: &str) -> Result<()> {
        let job = self.get_job(owner, &intent.job_id)?;
        let agent = self.store.agent(owner, &job.plan.executor_id)?;
        let invoice: ServiceInvoice = serde_json::from_value(
            intent.approval.as_ref().ok_or_else(ApiError::internal)?["invoice"].clone(),
        )?;
        self.event(
            &agent,
            "payment",
            "signed provider invoice paid",
            "confirmed",
            Some(&invoice.service_id),
            Some(&invoice.amount.to_string()),
            Some("USDT"),
            Some(hash),
        )
    }
}
