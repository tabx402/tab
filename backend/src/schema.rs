use crate::models::*;
use serde_json::{json, Value};
use utoipa::OpenApi;
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Tab API",
        version = "0.2.0",
        description = "Rust API for BNB Smart Chain agents, USDT jobs and verified receipts."
    ),
    components(schemas(
        Provider,
        AccountInput,
        AgentPlanInput,
        AgentPlan,
        AgentInput,
        RuntimeAgent,
        WalletChallenge,
        WalletProof,
        AgentEvent,
        AgentRun,
        PublicAgent,
        AgentRecord,
        PublicConfig,
        Health,
        RegisteredAgent,
        RegistryTransaction,
        RegistryData,
        crate::registry_cache::RegistryProgress,
        PurchaseSignature,
        PurchaseQuote,
        PurchaseResult,
        RegistrationSignature,
        SponsorshipResult,
        SponsoredChallenge,
        PauseInput,
        RunInput,
        JobMerchant,
        ServiceInvoice,
        JobInput,
        Job,
        PublicJob,
        JobSystem,
        JobAction,
        JobIntent,
        JobConfirmation,
        JobActionRecord,
        BountyInput,
        Bounty,
        FinancialInput,
        TokenMetadata,
        TransactionIntent,
        EvmTransaction,
        crate::starter::StarterObservation,
        crate::finance::FinanceInput,
        crate::finance::FinanceSystem,
        crate::finance::FinanceModule,
        crate::finance::FinanceAsset,
        crate::finance::FinanceModules,
        crate::finance::FinanceRequest,
        crate::job_execution::JobRun,
        crate::holder_access::HolderAccess,
        crate::holder_access::HolderChallenge,
        crate::holder_access::WalletInput,
        crate::holder_access::ProofInput
    ))
)]
struct ApiDoc;
pub fn document() -> Value {
    let mut doc = serde_json::to_value(ApiDoc::openapi()).expect("OpenAPI serializes");
    doc["components"]["schemas"]["AgentRun"]["properties"]["output"] =
        json!({"type":"object","additionalProperties":true});
    doc["components"]["schemas"]["JobRun"]["properties"]["output"] =
        json!({"type":"object","additionalProperties":true});
    doc["components"]["schemas"]["Job"]["allOf"][1]["properties"]["evidence"] =
        json!({"type":["object","null"],"additionalProperties":true});
    doc["components"]["schemas"]["AgentEvent"]["properties"]["preview"] =
        json!({"type":["object","null"],"additionalProperties":true});
    doc["components"]["schemas"]["TransactionIntent"]["properties"]["details"] =
        json!({"type":"object","additionalProperties":true});
    let operations: Vec<(&str, &str, Option<&str>, Option<&str>, bool)> = vec![
        (
            "get",
            "/api/account/holder-access",
            None,
            Some("HolderAccess"),
            true,
        ),
        (
            "post",
            "/api/account/holder-access/challenge",
            Some("WalletInput"),
            Some("HolderChallenge"),
            true,
        ),
        (
            "post",
            "/api/account/holder-access/verify",
            Some("ProofInput"),
            Some("HolderAccess"),
            true,
        ),
        ("get", "/api/account/profile", None, None, true),
        (
            "post",
            "/api/account/profile",
            Some("AccountInput"),
            None,
            true,
        ),
        ("get", "/api/credit/assets", None, None, false),
        (
            "get",
            "/api/starter/wallet",
            None,
            Some("StarterObservation"),
            false,
        ),
        ("get", "/api/operators", None, None, false),
        (
            "get",
            "/api/finance/system",
            None,
            Some("FinanceSystem"),
            false,
        ),
        (
            "post",
            "/api/finance/quote",
            Some("FinanceInput"),
            None,
            false,
        ),
        ("get", "/api/account/runtime/{id}/finance", None, None, true),
        (
            "post",
            "/api/account/runtime/{id}/finance/prepare",
            Some("FinanceInput"),
            Some("TransactionIntent"),
            true,
        ),
        (
            "get",
            "/api/account/runtime/{id}/finance/requests",
            None,
            None,
            true,
        ),
        (
            "post",
            "/api/account/runtime/{id}/finance/requests",
            Some("FinanceInput"),
            Some("FinanceRequest"),
            true,
        ),
        ("get", "/api/agents/live/{id}/runs/{run}", None, None, false),
        ("get", "/api/sponsorship", None, None, false),
        ("get", "/api/account/runtime/{id}/credit", None, None, true),
        (
            "get",
            "/api/account/runtime/{id}/wallet-actions",
            None,
            Some("TransactionIntent[]"),
            true,
        ),
        (
            "post",
            "/api/account/wallet-actions/{id}/submitted",
            Some("JobConfirmation"),
            None,
            true,
        ),
        (
            "post",
            "/api/account/wallet-actions/{id}/release-failed",
            None,
            None,
            true,
        ),
        (
            "post",
            "/api/account/job-actions/{id}/submitted",
            Some("JobConfirmation"),
            None,
            true,
        ),
        (
            "post",
            "/api/account/job-actions/{id}/release-failed",
            None,
            None,
            true,
        ),
        ("get", "/api/health", None, Some("Health"), false),
        ("get", "/api/config", None, Some("PublicConfig"), false),
        ("get", "/api/providers", None, Some("Provider[]"), false),
        ("get", "/api/registry", None, Some("RegistryData"), false),
        (
            "get",
            "/api/account/agents",
            None,
            Some("AgentPlan[]"),
            true,
        ),
        (
            "post",
            "/api/account/agents",
            Some("AgentPlanInput"),
            Some("AgentPlan"),
            true,
        ),
        ("delete", "/api/account/agents/{id}", None, None, true),
        (
            "get",
            "/api/account/runtime",
            None,
            Some("RuntimeAgent[]"),
            true,
        ),
        (
            "post",
            "/api/account/runtime",
            Some("AgentInput"),
            Some("RuntimeAgent"),
            true,
        ),
        (
            "patch",
            "/api/account/runtime/{id}",
            Some("AgentInput"),
            Some("RuntimeAgent"),
            true,
        ),
        (
            "post",
            "/api/account/runtime/{id}/challenge",
            Some("WalletChallenge"),
            None,
            true,
        ),
        (
            "post",
            "/api/account/runtime/{id}/register",
            Some("WalletProof"),
            Some("RuntimeAgent"),
            true,
        ),
        (
            "post",
            "/api/account/runtime/{id}/sponsor",
            Some("RegistrationSignature"),
            Some("SponsorshipResult"),
            true,
        ),
        (
            "post",
            "/api/account/runtime/{id}/sponsor/status",
            None,
            Some("SponsorshipResult"),
            true,
        ),
        (
            "post",
            "/api/account/runtime/{id}/pause",
            Some("PauseInput"),
            Some("RuntimeAgent"),
            true,
        ),
        (
            "post",
            "/api/account/runtime/{id}/run",
            Some("RunInput"),
            Some("AgentRun"),
            true,
        ),
        (
            "get",
            "/api/account/runtime/{id}/runs",
            None,
            Some("AgentRun[]"),
            true,
        ),
        (
            "get",
            "/api/account/runtime/{id}/events",
            None,
            Some("AgentEvent[]"),
            true,
        ),
        ("post", "/api/account/runtime/{id}/key", None, None, true),
        ("delete", "/api/account/runtime/{id}/key", None, None, true),
        ("post", "/api/agent/run", Some("RunInput"), Some("AgentRun"), true),
        ("get", "/api/activity", None, Some("AgentEvent[]"), false),
        (
            "post",
            "/api/account/runtime/{id}/purchase/quote",
            None,
            Some("PurchaseQuote"),
            true,
        ),
        (
            "post",
            "/api/account/runtime/{id}/purchase/{quote_id}",
            Some("PurchaseSignature"),
            Some("PurchaseResult"),
            true,
        ),
        (
            "post",
            "/api/account/runtime/{id}/purchase/{quote_id}/status",
            None,
            Some("PurchaseResult"),
            true,
        ),
        ("get", "/api/account/runtime/{id}/balance", None, None, true),
        (
            "get",
            "/api/account/runtime/{id}/purchases",
            None,
            Some("PurchaseResult[]"),
            true,
        ),
        (
            "get",
            "/api/agents/live",
            None,
            Some("PublicAgent[]"),
            false,
        ),
        (
            "get",
            "/api/agents/records",
            None,
            Some("AgentRecord[]"),
            false,
        ),
        ("get", "/api/agents/live/{id}", None, None, false),
        ("get", "/api/tools", None, None, false),
        ("get", "/api/capabilities", None, None, false),
        ("get", "/api/jobs/system", None, Some("JobSystem"), false),
        (
            "get",
            "/api/jobs/services",
            None,
            Some("JobMerchant[]"),
            false,
        ),
        ("get", "/api/jobs", None, Some("PublicJob[]"), false),
        ("get", "/api/account/jobs", None, Some("Job[]"), true),
        (
            "get",
            "/api/account/job-actions",
            None,
            Some("JobActionRecord[]"),
            true,
        ),
        ("get", "/api/account/jobs/{id}", None, Some("Job"), true),
        ("post", "/api/account/jobs/{id}/run", None, Some("JobRun"), true),
        ("get", "/api/account/jobs/{id}/runs", None, Some("JobRun[]"), true),
        (
            "post",
            "/api/account/runtime/{id}/jobs",
            Some("JobInput"),
            Some("Job"),
            true,
        ),
        (
            "post",
            "/api/account/jobs/{id}/branches",
            Some("JobInput"),
            Some("Job"),
            true,
        ),
        (
            "post",
            "/api/account/jobs/{id}/evidence",
            None,
            Some("Job"),
            true,
        ),
        (
            "put",
            "/api/account/jobs/{id}/evidence",
            None,
            Some("Job"),
            true,
        ),
        (
            "post",
            "/api/account/jobs/{id}/cancel-draft",
            None,
            Some("Job"),
            true,
        ),
        (
            "post",
            "/api/account/jobs/{id}/prepare",
            Some("JobAction"),
            Some("JobIntent"),
            true,
        ),
        ("get", "/api/agent/jobs", None, Some("Job[]"), true),
        ("post", "/api/agent/jobs/{id}/run", None, Some("JobRun"), true),
        ("get", "/api/agent/jobs/{id}/runs", None, Some("JobRun[]"), true),
        (
            "post",
            "/api/agent/jobs/{id}/branches",
            Some("JobInput"),
            Some("Job"),
            true,
        ),
        (
            "post",
            "/api/agent/jobs/{id}/evidence",
            None,
            Some("Job"),
            true,
        ),
        (
            "post",
            "/api/account/job-actions/{id}/confirm",
            Some("JobConfirmation"),
            Some("Job"),
            true,
        ),
        (
            "post",
            "/api/account/jobs/{id}/refresh",
            None,
            Some("Job"),
            true,
        ),
        (
            "get",
            "/api/bnb/transactions/{signature}",
            None,
            None,
            false,
        ),
        ("get", "/api/feed", None, None, false),
        ("get", "/api/bounties", None, Some("Bounty[]"), false),
        ("get", "/api/models", None, None, false),
        (
            "get",
            "/api/bnb/tokens/{address}",
            None,
            Some("TokenMetadata"),
            false,
        ),
        ("get", "/api/metrics", None, None, false),
        ("get", "/api/token/system", None, None, false),
        (
            "get",
            "/api/account/runtime/{id}/eligibility",
            None,
            None,
            true,
        ),
        ("get", "/api/account/bounties", None, Some("Bounty[]"), true),
        (
            "post",
            "/api/account/bounties",
            Some("BountyInput"),
            Some("Bounty"),
            true,
        ),
        (
            "post",
            "/api/account/runtime/{id}/wallet-actions/prepare",
            Some("FinancialInput"),
            Some("TransactionIntent"),
            true,
        ),
        (
            "post",
            "/api/account/wallet-actions/{id}/confirm",
            Some("JobConfirmation"),
            None,
            true,
        ),
        ("get", "/api/x402/system", None, None, false),
        ("get", "/api/account/runtime/{id}/x402", None, None, true),
        (
            "post",
            "/api/account/runtime/{id}/x402/{quote_id}/execute",
            Some("PurchaseSignature"),
            None,
            true,
        ),
        (
            "post",
            "/api/account/runtime/{id}/x402/{quote_id}/reconcile",
            Some("JobConfirmation"),
            None,
            true,
        ),
        (
            "post",
            "/api/account/runtime/{id}/x402/{quote_id}/release-expired",
            None,
            None,
            true,
        ),
        (
            "post",
            "/api/account/runtime/{id}/x402/quote",
            None,
            None,
            true,
        ),
    ];
    doc["paths"] = json!({});
    doc["components"]["securitySchemes"] = json!({"BearerAuth":{"type":"http","scheme":"bearer","bearerFormat":"JWT or scoped agent key"}});
    for (method, path, input, output, private) in operations {
        let success = if method == "delete" {
            "204"
        } else if method == "post"
            && (path == "/api/account/agents"
                || path == "/api/account/runtime"
                || path.ends_with("/jobs")
                || path.ends_with("/branches")
                || path == "/api/account/bounties")
        {
            "201"
        } else {
            "200"
        };
        let mut operation = json!({"responses":{success:{"description":"Successful response"},"401":{"description":"Authentication required"},"422":{"description":"Invalid input"},"503":{"description":"An integration is not configured or unavailable"}}});
        if let Some(model) = output {
            operation["responses"][success]["content"] =
                json!({"application/json":{"schema":reference(model)}});
        }
        if let Some(model) = input {
            operation["requestBody"] = json!({"required":model != "RunInput","content":{"application/json":{"schema":reference(model)}}});
        }
        if private {
            operation["security"] = json!([{"BearerAuth":[]}]);
            operation["responses"]["403"] = json!({"description":"The account, wallet, or current TAB holding does not authorize this action"});
        }
        let mut params = path
            .split('/')
            .filter_map(|part| part.strip_prefix('{')?.strip_suffix('}'))
            .map(|name| json!({"name":name,"in":"path","required":true,"schema":{"type":"string"}}))
            .collect::<Vec<_>>();
        if path == "/api/starter/wallet" {
            params.push(json!({"name":"address","in":"query","required":true,"schema":{"type":"string","pattern":"^0x[0-9a-fA-F]{40}$"}}));
        }
        if path == "/api/account/holder-access" {
            params.push(json!({"name":"wallet","in":"query","required":false,"schema":{"type":"string","pattern":"^0x[0-9a-fA-F]{40}$"}}));
            operation["responses"]["403"] =
                json!({"description":"Wallet ownership or TAB holding is required"});
        }
        if method == "post" && ["/api/account/agents", "/api/account/runtime", "/api/account/bounties"].contains(&path) {
            params.push(json!({"name":"X-Tab-Holder-Wallet","in":"header","required":false,"description":"Selected wallet; ownership and current TAB holdings are verified by the server when holder access is enabled","schema":{"type":"string","pattern":"^0x[0-9a-fA-F]{40}$"}}));
        }
        if !params.is_empty() {
            operation["parameters"] = json!(params);
        }
        doc["paths"][path][method] = operation;
    }
    for path in ["/api/backing/assets", "/api/assets/backing"] {
        doc["paths"][path]["get"] = json!({"responses":{"200":{"description":"Verified BNB token catalog; custody requires current code and implementation verification"}}});
    }
    doc
}
fn reference(name: &str) -> Value {
    if let Some(name) = name.strip_suffix("[]") {
        json!({"type":"array","items":{"$ref":format!("#/components/schemas/{name}")}})
    } else {
        json!({"$ref":format!("#/components/schemas/{name}")})
    }
}
