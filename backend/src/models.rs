use crate::error::{ApiError, Result};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

pub fn now() -> String {
    Utc::now().to_rfc3339()
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountInput {
    #[serde(default)]
    pub display_name: String,
}
pub fn identifier() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
pub fn digest(value: &impl Serialize) -> String {
    use sha2::{Digest, Sha256};
    // serde_json's map uses sorted keys; JSON values ensure canonical ordering.
    let canonical = serde_json::to_value(value).expect("serializable model");
    format!(
        "0x{}",
        hex::encode(Sha256::digest(
            serde_json::to_vec(&canonical).expect("canonical JSON")
        ))
    )
}
pub fn clean(value: &str, minimum: usize, maximum: usize) -> Result<String> {
    let value = value.trim();
    if value.chars().count() < minimum
        || value.chars().count() > maximum
        || value.chars().any(|c| c.is_control())
    {
        return Err(ApiError::validation(
            "Use plain text within the field length limits.",
        ));
    }
    Ok(value.into())
}
pub fn units(amount: Decimal) -> Result<u128> {
    token_units(amount, 18)
}
pub fn token_units(amount: Decimal, decimals: u8) -> Result<u128> {
    use rust_decimal::prelude::ToPrimitive;
    if amount < Decimal::ZERO || amount.scale() > u32::from(decimals) || decimals > 18 {
        return Err(ApiError::validation(
            "Use a nonnegative exact amount within the token precision.",
        ));
    }
    amount
        .checked_mul(Decimal::from(10u64.pow(decimals.into())))
        .and_then(|v| v.to_u128())
        .ok_or_else(|| ApiError::validation("Amount exceeds supported limits."))
}
pub fn money(amount: u128) -> Decimal {
    Decimal::from_i128_with_scale(amount.try_into().expect("bounded token amount"), 18).normalize()
}
pub fn usd_micros(amount: Decimal) -> Result<u64> {
    use rust_decimal::prelude::ToPrimitive;
    if amount < Decimal::ZERO {
        return Err(ApiError::validation("Budget must be nonnegative."));
    }
    amount
        .checked_mul(Decimal::from(1_000_000))
        .and_then(|v| v.floor().to_u64())
        .ok_or_else(|| ApiError::validation("Budget exceeds inference metering limits."))
}
pub fn usd_money(amount: u64) -> Decimal {
    Decimal::from(amount) / Decimal::from(1_000_000)
}
pub const TOOLS: &[&str] = &["bnb-rpc", "openrouter", "tavily", "web-search", "x402"];
pub fn tool_bitmap(tools: &[String]) -> u64 {
    TOOLS
        .iter()
        .enumerate()
        .filter(|(_, tool)| tools.iter().any(|selected| selected == *tool))
        .fold(0, |bits, (index, _)| bits | (1 << index))
}
pub const MODELS: &[&str] = &[
    "openai/gpt-4.1-mini",
    "openai/gpt-4.1",
    "anthropic/claude-sonnet-4",
    "google/gemini-2.5-flash",
    "deepseek/deepseek-chat-v3-0324",
];
fn manual() -> String {
    "manual".into()
}
fn onchain() -> String {
    "onchain".into()
}
fn default_model() -> String {
    MODELS[0].into()
}
fn yes() -> bool {
    true
}
fn report_id() -> u64 {
    437
}
fn buyer_review() -> String {
    "buyer_review".into()
}
fn one() -> u64 {
    1
}
fn full_penalty() -> u16 {
    10000
}

#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub website: String,
    pub status: String,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentPlanInput {
    pub name: String,
    pub purpose: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type=String)]
    pub daily_cap: Decimal,
    pub providers: Vec<String>,
}
impl AgentPlanInput {
    pub fn validate(mut self) -> Result<Self> {
        self.name = clean(&self.name, 2, 48)?;
        self.purpose = clean(&self.purpose, 5, 240)?;
        if self.daily_cap < Decimal::ONE
            || self.daily_cap > Decimal::from(10000)
            || self.providers.is_empty()
            || self.providers.len() > 8
            || !distinct(&self.providers)
        {
            return Err(ApiError::validation(
                "Choose distinct providers and a daily limit from 1 to 10,000.",
            ));
        }
        units(self.daily_cap)?;
        Ok(self)
    }
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentPlan {
    #[serde(flatten)]
    pub plan: AgentPlanInput,
    pub id: String,
    pub created_at: String,
    pub status: String,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentInput {
    pub name: String,
    pub purpose: String,
    #[serde(default = "onchain")]
    pub template: String,
    pub tools: Vec<String>,
    #[serde(default = "manual")]
    pub cadence: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type=String)]
    pub daily_cap: Decimal,
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub max_call: Decimal,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub watch_address: Option<String>,
    #[serde(default = "report_id")]
    pub report_agent: u64,
    #[serde(default = "yes")]
    pub public_activity: bool,
    #[serde(default)]
    pub token_address: Option<String>,
}
impl AgentInput {
    pub fn validate(mut self) -> Result<Self> {
        self.name = clean(&self.name, 2, 48)?;
        self.purpose = clean(&self.purpose, 5, 600)?;
        if self.name.len() > 48 {
            return Err(ApiError::validation(
                "Use an agent name shorter than 48 bytes.",
            ));
        }
        if !["onchain", "research", "custom", "influencer", "buyer"]
            .contains(&self.template.as_str())
            || !["manual", "hourly", "daily"].contains(&self.cadence.as_str())
        {
            return Err(ApiError::validation(
                "Choose a supported template and schedule.",
            ));
        }
        if self.tools.is_empty()
            || self.tools.len() > 5
            || !distinct(&self.tools)
            || self
                .tools
                .iter()
                .any(|tool| !TOOLS.contains(&tool.as_str()))
        {
            return Err(ApiError::validation(
                "Choose distinct supported BNB Smart Chain agent tools.",
            ));
        }
        if self.daily_cap < Decimal::ONE
            || self.daily_cap > Decimal::from(10000)
            || self.max_call <= Decimal::ZERO
            || self.max_call > Decimal::TEN
        {
            return Err(ApiError::validation(
                "Keep daily and per-call spending within configured limits.",
            ));
        }
        units(self.daily_cap)?;
        units(self.max_call)?;
        if self.max_call > self.daily_cap {
            return Err(ApiError::validation(
                "The per-call spending cap cannot exceed the daily spending cap.",
            ));
        }
        if !MODELS.contains(&self.model.as_str()) {
            return Err(ApiError::validation(
                "Choose a model from the approved model directory.",
            ));
        }
        for value in [&mut self.watch_address, &mut self.token_address]
            .into_iter()
            .flatten()
        {
            *value = crate::bnb::address(value)?;
        }
        Ok(self)
    }
}
pub fn distinct(values: &[String]) -> bool {
    let set: std::collections::HashSet<_> = values.iter().collect();
    set.len() == values.len()
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct RuntimeAgent {
    #[serde(flatten)]
    pub plan: AgentInput,
    pub id: String,
    pub wallet: Option<String>,
    pub registry_address: String,
    pub registry_id: Option<String>,
    pub registration_tx: Option<String>,
    pub status: String,
    pub created_at: String,
    pub last_run: Option<String>,
    pub next_run: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct WalletChallenge {
    pub wallet: String,
    #[serde(default)]
    pub sponsored: Option<bool>,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct WalletProof {
    pub wallet: String,
    pub signature: String,
    pub tx_hash: String,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentEvent {
    pub id: i64,
    #[serde(default)]
    pub run_id: Option<String>,
    pub agent_id: String,
    pub agent: String,
    pub kind: String,
    pub status: String,
    pub timestamp: String,
    pub message: String,
    pub provider: Option<String>,
    pub amount: Option<String>,
    pub currency: Option<String>,
    pub tx_hash: Option<String>,
    #[serde(default)]
    pub preview: Option<Value>,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentRun {
    pub id: String,
    pub agent_id: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub output: Value,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct PublicAgent {
    pub id: String,
    pub name: String,
    pub registry_id: String,
    pub registry_address: String,
    pub wallet: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type=String)]
    pub daily_cap: Decimal,
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub max_call: Decimal,
    pub tools: Vec<String>,
    pub cadence: String,
    pub status: String,
    pub last_run: Option<String>,
    pub next_run: Option<String>,
    pub purpose: String,
    pub model: String,
    pub token_address: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentRecord {
    pub id: String,
    pub runs: u64,
    pub failures: u64,
    pub payments: u64,
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub paid: Decimal,
    pub last_event_id: i64,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct PublicConfig {
    pub holder_access_enabled: bool,
    pub app_id: Option<String>,
    pub financial_actions_enabled: bool,
    pub contracts_status: String,
    pub chain_id: u64,
    pub network: String,
    pub gas_sponsorship_enabled: bool,
    pub gas_sponsorship_status: String,
    pub gas_sponsorship_message: String,
    pub payments_enabled: bool,
    pub agent_execution_enabled: bool,
    pub usdt_address: String,
    pub usdt_decimals: u8,
    pub official_tab_address: Option<String>,
    pub preferred_wallet: String,
    pub backend: String,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct Health {
    pub status: String,
    pub financial_actions_enabled: bool,
    pub backend: String,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct RegisteredAgent {
    pub id: String,
    pub name: String,
    pub purpose: String,
    pub owner: String,
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub daily_cap: Decimal,
    pub providers: Vec<String>,
    pub paused: bool,
    pub version: u32,
    pub policy_hash: String,
    pub policy_matches: bool,
    pub funding_status: String,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct RegistryTransaction {
    pub label: String,
    pub hash: String,
    pub status: String,
    pub block_number: String,
    pub fee_bnb: String,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct RegistryData {
    pub status: String,
    pub chain_id: u64,
    pub network: String,
    pub address: Option<String>,
    pub owner: Option<String>,
    pub block_number: Option<String>,
    pub verified_at: Option<String>,
    pub agents: Vec<RegisteredAgent>,
    pub transactions: Vec<RegistryTransaction>,
    pub fees_bnb: Option<String>,
    pub error: Option<String>,
    pub discovery: crate::registry_cache::RegistryProgress,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct PurchaseSignature {
    pub signature: String,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct PurchaseQuote {
    pub chain_id: u64,
    pub id: String,
    pub provider: String,
    pub amount: String,
    pub currency: String,
    pub recipient: String,
    pub expires_at: u64,
    pub transaction: EvmTransaction,
    pub transactions: Vec<EvmTransaction>,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct PurchaseResult {
    pub id: String,
    pub status: String,
    pub amount: String,
    pub tx_hash: Option<String>,
    pub data: Option<Value>,
    pub message: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct RegistrationSignature {
    pub wallet: String,
    pub signature: String,
    pub request_id: String,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct SponsorshipResult {
    pub status: String,
    pub request_id: Option<String>,
    pub retryable: bool,
    pub tx_hash: Option<String>,
    pub agent: Option<RuntimeAgent>,
    pub message: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct SponsoredChallenge {
    pub sponsored: bool,
    pub request_id: String,
    pub wallet: String,
    pub registry_id: String,
    pub registry: String,
    pub policy_hash: String,
    pub chain_id: u64,
    pub expires_at: String,
    pub typed_data: Value,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct PauseInput {
    pub paused: bool,
}
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RunInput {
    pub quote_id: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct JobMerchant {
    pub id: String,
    pub name: String,
    pub tool: String,
    pub service_key: String,
    pub recipient: String,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceInvoice {
    pub service_id: String,
    pub request_hash: String,
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    pub expires: u64,
    pub nonce: String,
    pub signature: String,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct JobInput {
    pub title: String,
    pub description: String,
    pub executor_id: String,
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub budget: Decimal,
    #[schema(value_type=String)]
    pub max_call: Decimal,
    pub deadline: DateTime<Utc>,
    pub tools: Vec<String>,
    #[serde(default = "buyer_review")]
    pub acceptance: String,
    #[serde(default = "one")]
    pub minimum_block: u64,
    #[serde(default = "yes")]
    pub public_activity: bool,
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default, alias = "bond_amount")]
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub bond_tokens: Decimal,
    #[serde(default)]
    pub bond_token_address: Option<String>,
    #[serde(default)]
    pub penalty_rule: Option<String>,
    #[serde(default = "full_penalty")]
    pub penalty_bps: u16,
}
impl JobInput {
    pub fn validate(mut self) -> Result<Self> {
        self.title = clean(&self.title, 3, 120)?;
        self.description = clean(&self.description, 5, 1200)?;
        if self.executor_id.len() != 32 || hex::decode(&self.executor_id).is_err() {
            return Err(ApiError::validation("Use a valid executor identifier."));
        }
        if self.budget <= Decimal::ZERO
            || self.budget > Decimal::from(10000)
            || self.max_call <= Decimal::ZERO
            || self.max_call > Decimal::TEN
            || self.max_call > self.budget
        {
            return Err(ApiError::validation(
                "Use a per-call limit within the USDT job budget.",
            ));
        }
        units(self.budget)?;
        units(self.max_call)?;
        if self.tools.is_empty()
            || self.tools.len() > 5
            || !distinct(&self.tools)
            || self
                .tools
                .iter()
                .any(|tool| !TOOLS.contains(&tool.as_str()))
            || !distinct(&self.services)
            || self.services.len() > 32
        {
            return Err(ApiError::validation(
                "Choose distinct supported tools and services.",
            ));
        }
        if self.acceptance != "buyer_review" {
            return Err(ApiError::validation(
                "Job delivery uses buyer review. Subjective feedback cannot slash collateral.",
            ));
        }
        let current = Utc::now();
        if self.deadline < current + chrono::Duration::minutes(5)
            || self.deadline > current + chrono::Duration::days(30)
        {
            return Err(ApiError::validation(
                "Choose a deadline between five minutes and thirty days away.",
            ));
        }
        if self.bond_tokens < Decimal::ZERO
            || self.bond_tokens.scale() > 18
            || !(1..=10000).contains(&self.penalty_bps)
        {
            return Err(ApiError::validation("Use a nonnegative token bond and an explicit penalty rate from 1 to 10,000 basis points."));
        }
        if self.bond_tokens > Decimal::ZERO {
            crate::bnb::pubkey(self.bond_token_address.as_deref().ok_or_else(|| {
                ApiError::validation("Choose the exact contract address of a token bond.")
            })?)?;
            if self.penalty_rule.as_deref() != Some("deadline_missed") {
                return Err(ApiError::validation("Bond terms require the objective deadline_missed rule. Buyer disagreement never burns stake."));
            }
        }
        Ok(self)
    }
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct Job {
    #[serde(flatten)]
    pub plan: JobInput,
    pub id: String,
    pub root_id: String,
    pub parent_id: Option<String>,
    pub requester_id: String,
    pub buyer_wallet: String,
    pub executor_wallet: String,
    pub executor_name: String,
    pub created_at: String,
    pub depth: u8,
    pub terms_hash: String,
    pub state: String,
    pub funding: String,
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub available: Decimal,
    pub evidence_hash: Option<String>,
    pub evidence: Option<Value>,
    pub paused: bool,
    pub chain_tx: Option<String>,
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub provider_paid: Decimal,
    #[schema(value_type=String)]
    pub reward_paid: Decimal,
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub refunded: Decimal,
    pub root_services: Vec<JobMerchant>,
    #[serde(default)]
    pub bond_status: String,
    #[serde(default)]
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub fee_paid: Decimal,
    #[serde(default)]
    pub timely_submitted: bool,
    #[serde(default)]
    pub submitted_at: Option<i64>,
    #[serde(default)]
    pub cancelled_at: Option<i64>,
    #[serde(default)]
    pub terms: Value,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct PublicJob {
    pub id: String,
    pub root_id: String,
    pub parent_id: Option<String>,
    pub requester_id: String,
    pub executor_id: String,
    pub executor_name: String,
    pub title: String,
    pub acceptance: String,
    pub deadline: DateTime<Utc>,
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub budget: Decimal,
    #[schema(value_type=String)]
    pub available: Decimal,
    pub state: String,
    pub funding: String,
    pub terms_hash: String,
    pub evidence_hash: Option<String>,
    pub chain_tx: Option<String>,
    pub paused: bool,
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub reward_paid: Decimal,
    #[schema(value_type=String)]
    pub provider_paid: Decimal,
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub bond_tokens: Decimal,
    pub bond_token_address: Option<String>,
    pub bond_status: String,
    pub penalty_rule: Option<String>,
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub fee_paid: Decimal,
    pub timely_submitted: bool,
    pub submitted_at: Option<i64>,
    pub cancelled_at: Option<i64>,
    pub penalty_bps: u16,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct JobSystem {
    pub chain_id: u64,
    pub network: String,
    pub escrow: Option<String>,
    pub token: String,
    pub status: String,
    pub max_depth: u8,
    pub review_window_seconds: u64,
    pub service_payments_enabled: bool,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct JobAction {
    pub action: String,
    #[serde(default)]
    pub invoice: Option<ServiceInvoice>,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct JobIntent {
    #[serde(default)]
    pub tx_hash: Option<String>,
    pub id: String,
    pub job_id: String,
    pub action: String,
    pub chain_id: u64,
    pub network: String,
    pub sender: String,
    pub to: String,
    pub data: String,
    pub value: String,
    pub approval: Option<Value>,
    pub transactions: Vec<EvmTransaction>,
    pub expires_at: String,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct JobConfirmation {
    pub tx_hash: String,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct JobActionRecord {
    pub intent: JobIntent,
    pub tx_hash: Option<String>,
    pub confirmed: u8,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct BountyInput {
    pub title: String,
    pub description: String,
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub budget: Decimal,
    pub deadline: DateTime<Utc>,
    pub tools: Vec<String>,
    #[serde(default)]
    pub public_activity: bool,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct Bounty {
    #[serde(flatten)]
    pub plan: BountyInput,
    pub id: String,
    pub created_at: String,
    pub status: String,
    pub funding: String,
    pub assigned_agent_id: Option<String>,
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub chain_tx: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FinancialInput {
    pub action: String,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[schema(value_type=Option<String>)]
    pub minimum_collateral_out: Option<Decimal>,
    #[serde(default)]
    pub credit_id: Option<String>,
    #[serde(default)]
    pub recipient: Option<String>,
    #[serde(default)]
    pub request_hash: Option<String>,
    #[serde(default)]
    pub receipt_hash: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub symbol: Option<String>,

    #[serde(default)]
    pub token_address: Option<String>,
    #[serde(default)]
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    #[serde(default)]
    pub lock_seconds: Option<i64>,
    #[serde(default)]
    pub metadata_hash: Option<String>,
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub bounty_id: Option<String>,
    #[serde(default)]
    pub target_agent_id: Option<String>,
    #[serde(default)]
    pub penalty_bps: Option<u16>,
    #[serde(default)]
    pub side: Option<bool>,
    #[serde(default)]
    pub closes_at: Option<i64>,
    #[serde(default)]
    pub paused: Option<bool>,
    #[serde(default, with = "optional_u64_string")]
    #[schema(value_type=Option<String>)]
    pub session_nonce: Option<u64>,
    #[serde(default)]
    pub session_signer: Option<String>,
    #[serde(default)]
    pub session_expires_at: Option<i64>,
    #[serde(default)]
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str_option")]
    pub per_call: Option<Decimal>,
    #[serde(default)]
    #[schema(value_type=String)]
    #[serde(with = "rust_decimal::serde::str_option")]
    pub daily_cap: Option<Decimal>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub recipients: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct TokenMetadata {
    pub chain_id: u64,
    pub address: String,
    pub decimals: u8,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct TransactionIntent {
    #[serde(default)]
    pub tx_hash: Option<String>,
    pub id: String,
    pub agent_id: String,
    pub action: String,
    pub chain_id: u64,
    pub network: String,
    pub sender: String,
    pub to: String,
    pub data: String,
    pub value: String,
    pub transaction: EvmTransaction,
    pub transactions: Vec<EvmTransaction>,
    pub expires_at: String,
    pub details: Value,
}
mod optional_u64_string {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(
        value: &Option<u64>,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        value.map(|value| value.to_string()).serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Option<u64>, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.is_null() {
            return Ok(None);
        }
        value
            .as_str()
            .and_then(|value| value.parse().ok())
            .or_else(|| value.as_u64())
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("Use an exact u64 session nonce string."))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct EvmTransaction {
    pub to: String,
    pub data: String,
    pub value: String,
    #[serde(rename = "chainId")]
    pub chain_id: String,
}
