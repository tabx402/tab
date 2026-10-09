//! x402 v2 exact EVM transport. Wallet signatures and confirmed token transfers
//! are separate checks; HTTP success alone never proves a payment.
use crate::error::{ApiError, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use ethabi::{
    ethereum_types::{H160, U256},
    Token,
};
use reqwest::{redirect::Policy, Client, Response, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::time::Duration;
pub const NETWORK: &str = "eip155:56";
pub const USDT: &str = "0x55d398326f99059fF775485246999027B3197955";
pub const PERMIT2: &str = "0x000000000022D473030F116dDEE9F6B43aC78BA3";
pub const EXACT_PROXY: &str = "0x402085c248EeA27D92E8b30b2C58ed07f9E20001";
const MAX_HEADER: usize = 32768;
const MAX_BODY: usize = 65536;
const MAX_AUTHORIZATION_SECONDS: u64 = 120;
mod units {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
        let s = String::deserialize(d)?;
        super::amount(&s).map_err(|e| serde::de::Error::custom(e.1))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Merchant {
    pub id: String,
    pub resource_url: String,
    pub facilitator_url: String,
    pub recipient: String,
    pub network: String,
    pub asset: String,
    #[serde(default)]
    pub fee_payer: String,
    #[serde(with = "units")]
    pub max_amount: u128,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub url: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Requirements {
    pub scheme: String,
    pub network: String,
    pub amount: String,
    pub asset: String,
    pub pay_to: String,
    pub max_timeout_seconds: u64,
    #[serde(default)]
    pub extra: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequired {
    pub x402_version: u8,
    pub resource: Resource,
    pub accepts: Vec<Requirements>,
}
#[derive(Clone, Copy)]
pub struct Limits {
    pub per_call: u128,
    pub daily_remaining: u128,
    pub total_remaining: u128,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Offer {
    pub resource: Resource,
    pub accepted: Requirements,
    #[serde(with = "units")]
    pub amount_units: u128,
    pub request_hash: String,
    pub status: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settlement {
    pub success: bool,
    pub transaction: String,
    pub network: String,
    pub payer: String,
}
fn address(s: &str) -> Result<H160> {
    if s.len() != 42 || !s.starts_with("0x") {
        return Err(ApiError::bad("Invalid EVM address."));
    }
    s.parse().map_err(|_| ApiError::bad("Invalid EVM address."))
}
fn same_address(a: &str, b: &str) -> bool {
    address(a)
        .ok()
        .zip(address(b).ok())
        .is_some_and(|(a, b)| a == b)
}
fn hash(s: &str) -> Result<[u8; 32]> {
    hex::decode(s.strip_prefix("0x").unwrap_or(""))
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| ApiError::bad("Invalid transaction hash."))
}
fn amount(s: &str) -> Result<u128> {
    if s.is_empty()
        || s.len() > 39
        || !s.bytes().all(|b| b.is_ascii_digit())
        || (s.len() > 1 && s.starts_with('0'))
    {
        return Err(ApiError::bad("Use a canonical integer token amount."));
    }
    s.parse()
        .map_err(|_| ApiError::bad("Token amount exceeds supported limits."))
}
fn uint(s: &str) -> Result<U256> {
    if s.is_empty()
        || s.len() > 78
        || !s.bytes().all(|b| b.is_ascii_digit())
        || (s.len() > 1 && s.starts_with('0'))
    {
        return Err(ApiError::bad("Invalid unsigned integer."));
    }
    U256::from_dec_str(s).map_err(|_| ApiError::bad("Invalid unsigned integer."))
}
fn keccak(b: impl AsRef<[u8]>) -> [u8; 32] {
    Keccak256::digest(b.as_ref()).into()
}
fn bytes32(v: [u8; 32]) -> Token {
    Token::FixedBytes(v.to_vec())
}
impl Merchant {
    pub fn validate(&self, network: &str, asset: &str) -> Result<()> {
        public_https(&self.resource_url)?;
        let facilitator = public_https(&self.facilitator_url)?;
        if facilitator.query().is_some()
            || self.id.is_empty()
            || self.id.len() > 64
            || self.max_amount == 0
        {
            return Err(ApiError::validation("Invalid merchant configuration."));
        }
        if address(&self.recipient)? == H160::zero() {
            return Err(ApiError::validation("Merchant needs a nonzero recipient."));
        }
        if !self.fee_payer.is_empty() {
            address(&self.fee_payer)?;
        }
        if network != NETWORK
            || self.network != NETWORK
            || !same_address(asset, USDT)
            || !same_address(&self.asset, USDT)
        {
            return Err(ApiError::validation(
                "Merchant must use BNB Chain 56 and its configured USDT token.",
            ));
        }
        Ok(())
    }
}
pub fn validate_offer(header: &str, merchant: &Merchant, limits: Limits) -> Result<Offer> {
    merchant.validate(NETWORK, USDT)?;
    if header.len() > MAX_HEADER {
        return Err(ApiError::bad("Payment requirements are too large."));
    }
    let bytes = STANDARD
        .decode(header)
        .map_err(|_| ApiError::bad("Invalid PAYMENT-REQUIRED encoding."))?;
    let required: PaymentRequired = serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::bad("Invalid x402 payment requirements."))?;
    if required.x402_version != 2
        || required.accepts.is_empty()
        || required.accepts.len() > 16
        || required.resource.url != merchant.resource_url
    {
        return Err(ApiError::bad(
            "Payment requirements do not match this resource.",
        ));
    }
    let cap = merchant
        .max_amount
        .min(limits.per_call)
        .min(limits.daily_remaining)
        .min(limits.total_remaining);
    let (amount_units, accepted) = required
        .accepts
        .into_iter()
        .filter_map(|item| {
            let n = amount(&item.amount).ok()?;
            if item.scheme != "exact"
                || item.network != NETWORK
                || !same_address(&item.asset, USDT)
                || !same_address(&item.pay_to, &merchant.recipient)
                || item.extra["assetTransferMethod"] != "permit2"
                || n == 0
                || n > cap
                || item.max_timeout_seconds == 0
            {
                return None;
            }
            Some((n, item))
        })
        .min_by_key(|(n, _)| *n)
        .ok_or_else(|| {
            ApiError::forbidden("No approved USDT Permit2 offer fits the spending limits.")
        })?;
    let bound = json!({"resource":required.resource,"accepted":accepted,"merchant":merchant.id});
    let request_hash = hex::encode(keccak(
        serde_json::to_vec(&bound).map_err(|_| ApiError::internal())?,
    ));
    Ok(Offer {
        resource: required.resource,
        accepted,
        amount_units,
        request_hash,
        status: "requires_wallet_authorization".into(),
    })
}
pub async fn fetch_offer(
    merchant: &Merchant,
    network: &str,
    asset: &str,
    limits: Limits,
) -> Result<Offer> {
    merchant.validate(network, asset)?;
    let response = http_client()?
        .get(&merchant.resource_url)
        .send()
        .await
        .map_err(|_| ApiError::unavailable("Merchant is unavailable."))?;
    if response.status() != reqwest::StatusCode::PAYMENT_REQUIRED {
        return Err(ApiError::unavailable(
            "Merchant did not return x402 payment requirements.",
        ));
    }
    let header = response
        .headers()
        .get("PAYMENT-REQUIRED")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| ApiError::unavailable("Merchant did not provide PAYMENT-REQUIRED."))?;
    validate_offer(header, merchant, limits)
}
/// Capability discovery is not proof of allowance, funding or settlement.
pub async fn sponsor_supported(merchant: &Merchant) -> Result<bool> {
    merchant.validate(NETWORK, USDT)?;
    let response = http_client()?
        .get(format!(
            "{}/supported",
            merchant.facilitator_url.trim_end_matches('/')
        ))
        .send()
        .await
        .map_err(|_| ApiError::unavailable("Facilitator is unavailable."))?;
    if !response.status().is_success() {
        return Ok(false);
    }
    let body: Value = serde_json::from_slice(&bounded_body(response).await?)
        .map_err(|_| ApiError::unavailable("Invalid facilitator capabilities."))?;
    Ok(supports_merchant(&body, merchant))
}
fn supports_merchant(body: &Value, merchant: &Merchant) -> bool {
    let kind = body["kinds"].as_array().is_some_and(|kinds| {
        kinds.iter().any(|k| {
            k["x402Version"] == 2
                && k["scheme"] == "exact"
                && k["network"] == NETWORK
                // Discovery may omit this optional metadata. The actual payment
                // offer must still explicitly advertise the Permit2 transfer path.
                && k.get("extra")
                    .and_then(|extra| extra.get("assetTransferMethod"))
                    .is_none_or(|method| method == "permit2")
        })
    });
    let signer = merchant.fee_payer.is_empty()
        || [NETWORK, "eip155:*"].iter().any(|network| {
            body["signers"][*network].as_array().is_some_and(|keys| {
                keys.iter().any(|k| {
                    k.as_str()
                        .is_some_and(|s| same_address(s, &merchant.fee_payer))
                })
            })
        });
    kind && signer
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Authorization {
    pub permitted: Permission,
    pub from: String,
    pub spender: String,
    pub nonce: String,
    pub deadline: String,
    pub witness: Witness,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permission {
    pub token: String,
    pub amount: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Witness {
    pub to: String,
    pub valid_after: String,
}
impl Authorization {
    pub fn new(offer: &Offer, payer: &str, now: u64) -> Result<Self> {
        let nonce = U256::from_big_endian(&rand::random::<[u8; 32]>());
        let auth = Self {
            permitted: Permission {
                token: USDT.into(),
                amount: offer.amount_units.to_string(),
            },
            from: payer.into(),
            spender: EXACT_PROXY.into(),
            nonce: nonce.to_string(),
            deadline: now
                .saturating_add(
                    offer
                        .accepted
                        .max_timeout_seconds
                        .min(MAX_AUTHORIZATION_SECONDS),
                )
                .min((now / 86_400 + 1).saturating_mul(86_400).saturating_sub(1))
                .to_string(),
            witness: Witness {
                to: offer.accepted.pay_to.clone(),
                valid_after: now.saturating_sub(5).to_string(),
            },
        };
        auth.validate(offer, payer, now)?;
        Ok(auth)
    }
    pub fn validate(&self, offer: &Offer, payer: &str, now: u64) -> Result<()> {
        let deadline = uint(&self.deadline)?;
        let after = uint(&self.witness.valid_after)?;
        uint(&self.nonce)?;
        if !same_address(&self.from, payer)
            || address(payer)? == H160::zero()
            || !same_address(&self.spender, EXACT_PROXY)
            || !same_address(&self.permitted.token, USDT)
            || !same_address(&self.witness.to, &offer.accepted.pay_to)
            || self.permitted.amount != offer.amount_units.to_string()
            || offer.accepted.amount != self.permitted.amount
            || offer.accepted.scheme != "exact"
            || offer.accepted.network != NETWORK
            || offer.accepted.extra["assetTransferMethod"] != "permit2"
            || !same_address(&offer.accepted.asset, USDT)
            || deadline <= U256::from(now)
            || deadline
                > U256::from(
                    now.saturating_add(
                        offer
                            .accepted
                            .max_timeout_seconds
                            .min(MAX_AUTHORIZATION_SECONDS),
                    )
                    .min((now / 86_400 + 1).saturating_mul(86_400).saturating_sub(1)),
                )
            || after > U256::from(now)
            || deadline.saturating_sub(after) > U256::from(125)
        {
            return Err(ApiError::bad(
                "Permit2 authorization differs from the approved payment or has expired.",
            ));
        }
        Ok(())
    }
    pub fn typed_data(&self) -> Value {
        json!({"domain":{"name":"Permit2","chainId":56,"verifyingContract":PERMIT2},"primaryType":"PermitWitnessTransferFrom","types":{"EIP712Domain":[{"name":"name","type":"string"},{"name":"chainId","type":"uint256"},{"name":"verifyingContract","type":"address"}],"PermitWitnessTransferFrom":[{"name":"permitted","type":"TokenPermissions"},{"name":"spender","type":"address"},{"name":"nonce","type":"uint256"},{"name":"deadline","type":"uint256"},{"name":"witness","type":"Witness"}],"TokenPermissions":[{"name":"token","type":"address"},{"name":"amount","type":"uint256"}],"Witness":[{"name":"to","type":"address"},{"name":"validAfter","type":"uint256"}]},"message":{"permitted":self.permitted,"spender":self.spender,"nonce":self.nonce,"deadline":self.deadline,"witness":self.witness}})
    }
    fn digest(&self) -> Result<[u8; 32]> {
        let domain = keccak(ethabi::encode(&[
            bytes32(keccak(
                "EIP712Domain(string name,uint256 chainId,address verifyingContract)",
            )),
            bytes32(keccak("Permit2")),
            Token::Uint(U256::from(56)),
            Token::Address(address(PERMIT2)?),
        ]));
        let permission = keccak(ethabi::encode(&[
            bytes32(keccak("TokenPermissions(address token,uint256 amount)")),
            Token::Address(address(&self.permitted.token)?),
            Token::Uint(uint(&self.permitted.amount)?),
        ]));
        let witness = keccak(ethabi::encode(&[
            bytes32(keccak("Witness(address to,uint256 validAfter)")),
            Token::Address(address(&self.witness.to)?),
            Token::Uint(uint(&self.witness.valid_after)?),
        ]));
        let message=keccak(ethabi::encode(&[bytes32(keccak("PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,Witness witness)TokenPermissions(address token,uint256 amount)Witness(address to,uint256 validAfter)")),bytes32(permission),Token::Address(address(&self.spender)?),Token::Uint(uint(&self.nonce)?),Token::Uint(uint(&self.deadline)?),bytes32(witness)]));
        Ok(keccak([&[0x19, 0x01][..], &domain, &message].concat()))
    }
    pub fn verify_eoa_signature(&self, signature: &str) -> Result<()> {
        let bytes = hex::decode(signature.strip_prefix("0x").unwrap_or(""))
            .map_err(|_| ApiError::bad("Invalid wallet signature."))?;
        if bytes.len() != 65 {
            return Err(ApiError::bad("An EVM wallet signature is required."));
        }
        let sig = k256::ecdsa::Signature::from_slice(&bytes[..64])
            .map_err(|_| ApiError::bad("Invalid wallet signature."))?;
        if sig.normalize_s().is_some() {
            return Err(ApiError::bad("Noncanonical wallet signature."));
        }
        let v = match bytes[64] {
            0 | 1 => bytes[64],
            27 | 28 => bytes[64] - 27,
            _ => return Err(ApiError::bad("Invalid recovery id.")),
        };
        let key = k256::ecdsa::VerifyingKey::recover_from_prehash(
            &self.digest()?,
            &sig,
            k256::ecdsa::RecoveryId::from_byte(v).ok_or_else(ApiError::internal)?,
        )
        .map_err(|_| ApiError::bad("Invalid wallet signature."))?;
        let recovered = keccak(&key.to_encoded_point(false).as_bytes()[1..]);
        if H160::from_slice(&recovered[12..]) != address(&self.from)? {
            return Err(ApiError::forbidden(
                "Wallet signature belongs to a different payer.",
            ));
        }
        Ok(())
    }
    pub fn settlement_calldata(&self, signature: &str) -> Result<String> {
        self.verify_eoa_signature(signature)?;
        let args = ethabi::encode(&[
            Token::Tuple(vec![
                Token::Tuple(vec![
                    Token::Address(address(&self.permitted.token)?),
                    Token::Uint(uint(&self.permitted.amount)?),
                ]),
                Token::Uint(uint(&self.nonce)?),
                Token::Uint(uint(&self.deadline)?),
            ]),
            Token::Address(address(&self.from)?),
            Token::Tuple(vec![
                Token::Address(address(&self.witness.to)?),
                Token::Uint(uint(&self.witness.valid_after)?),
            ]),
            Token::Bytes(
                hex::decode(&signature[2..])
                    .map_err(|_| ApiError::bad("Invalid wallet signature."))?,
            ),
        ]);
        Ok(format!(
            "0x{}{}",
            hex::encode(
                &keccak(
                    "settle(((address,uint256),uint256,uint256),address,(address,uint256),bytes)"
                )[..4]
            ),
            hex::encode(args)
        ))
    }
}
pub fn payment_signature(
    offer: &Offer,
    auth: &Authorization,
    signature: &str,
    now: u64,
) -> Result<String> {
    auth.validate(offer, &auth.from, now)?;
    auth.verify_eoa_signature(signature)?;
    Ok(STANDARD.encode(serde_json::to_vec(&json!({"x402Version":2,"resource":offer.resource,"accepted":offer.accepted,"payload":{"signature":signature,"permit2Authorization":auth}})).map_err(|_|ApiError::internal())?))
}
pub fn settlement_response(header: &str, merchant: &Merchant) -> Result<Settlement> {
    if header.len() > MAX_HEADER {
        return Err(ApiError::bad("Settlement response is too large."));
    }
    let bytes = STANDARD
        .decode(header)
        .map_err(|_| ApiError::bad("Invalid PAYMENT-RESPONSE encoding."))?;
    let r: Settlement = serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::bad("Invalid settlement response."))?;
    if !r.success || r.network != merchant.network {
        return Err(ApiError::bad(
            "Settlement does not match the configured network.",
        ));
    }
    hash(&r.transaction)?;
    address(&r.payer)?;
    Ok(r)
}
/// Pinned BNB RPC must establish canonical block and finality before calling.
/// Exact calldata binds this receipt to the stored quote nonce, amount and payer.
pub fn verify_transfer(
    receipt: &Value,
    tx: &Value,
    expected_hash: &str,
    auth: &Authorization,
    signature: &str,
    merchant: &Merchant,
) -> Result<()> {
    hash(expected_hash)?;
    let calldata = auth.settlement_calldata(signature)?;
    if receipt["status"] != "0x1"
        || receipt["transactionHash"]
            .as_str()
            .is_none_or(|h| !h.eq_ignore_ascii_case(expected_hash))
        || tx["hash"]
            .as_str()
            .is_none_or(|h| !h.eq_ignore_ascii_case(expected_hash))
        || tx["chainId"] != "0x38"
        || tx["to"]
            .as_str()
            .is_none_or(|s| !same_address(s, EXACT_PROXY))
        || tx["input"]
            .as_str()
            .is_none_or(|s| !s.eq_ignore_ascii_case(&calldata))
        || (!merchant.fee_payer.is_empty()
            && tx["from"]
                .as_str()
                .is_none_or(|s| !same_address(s, &merchant.fee_payer)))
    {
        return Err(ApiError::bad(
            "Receipt does not prove the authorized Permit2 payment.",
        ));
    }
    let logs = receipt["logs"]
        .as_array()
        .ok_or_else(|| ApiError::bad("Receipt logs are unavailable."))?;
    let topic = format!(
        "0x{}",
        hex::encode(keccak("Transfer(address,address,uint256)"))
    );
    let source = format!("0x{:0>64}", &auth.from[2..]);
    let dest = format!("0x{:0>64}", &auth.witness.to[2..]);
    let n = format!("0x{:064x}", uint(&auth.permitted.amount)?);
    let matching = logs
        .iter()
        .filter(|l| {
            l["address"].as_str().is_some_and(|s| same_address(s, USDT))
                && l["topics"][0]
                    .as_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case(&topic))
                && l["topics"][1]
                    .as_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case(&source))
                && l["topics"][2]
                    .as_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case(&dest))
        })
        .collect::<Vec<_>>();
    if matching.len() != 1
        || matching[0]["removed"] == true
        || matching[0]["data"]
            .as_str()
            .is_none_or(|s| !s.eq_ignore_ascii_case(&n))
    {
        return Err(ApiError::bad(
            "Receipt must contain exactly the approved USDT transfer.",
        ));
    }
    Ok(())
}
/// Caller reserves the quote before this single HTTP attempt. An ambiguous
/// timeout must be reconciled with the stored nonce, never automatically repaid.
pub async fn retry_paid(merchant: &Merchant, header: &str) -> Result<(Vec<u8>, Settlement, bool)> {
    merchant.validate(NETWORK, USDT)?;
    if header.len() > MAX_HEADER {
        return Err(ApiError::bad("Payment signature is too large."));
    }
    let response = http_client()?
        .get(&merchant.resource_url)
        .header("PAYMENT-SIGNATURE", header)
        .send()
        .await
        .map_err(|_| {
            ApiError::unavailable(
                "Payment submission is uncertain; reconcile its nonce before retrying.",
            )
        })?;
    let settlement = response
        .headers()
        .get("PAYMENT-RESPONSE")
        .and_then(|s| s.to_str().ok())
        .ok_or_else(|| {
            ApiError::unavailable(
                "Merchant returned no settlement proof; reconcile before retrying.",
            )
        })?;
    let receipt = settlement_response(settlement, merchant)?;
    Ok(paid_body(response, receipt).await)
}
async fn paid_body(response: Response, receipt: Settlement) -> (Vec<u8>, Settlement, bool) {
    let service_ok = response.status().is_success();
    match bounded_body(response).await {
        Ok(body)=>(body,receipt,service_ok),
        Err(_)=>(b"Service response was unavailable; the payment receipt is retained for reconciliation.".to_vec(),receipt,false),
    }
}
fn public_https(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| ApiError::validation("Invalid merchant URL."))?;
    let host = url
        .host_str()
        .ok_or_else(|| ApiError::validation("Merchant URL needs a host."))?;
    let ip_host = host.trim_start_matches('[').trim_end_matches(']');
    let private = ip_host
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| match ip {
            std::net::IpAddr::V4(ip) => {
                ip.is_private()
                    || ip.is_loopback()
                    || ip.is_link_local()
                    || ip.is_unspecified()
                    || ip.is_broadcast()
                    || ip.is_multicast()
            }
            std::net::IpAddr::V6(ip) => {
                ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local()
                    || ip.is_multicast()
            }
        });
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || private
        || host == "localhost"
        || host.ends_with(".localhost")
    {
        return Err(ApiError::validation(
            "Use an operator-approved public HTTPS merchant endpoint.",
        ));
    }
    Ok(url)
}

fn http_client() -> Result<Client> {
    // Merchant redirects cannot escape the installed endpoint allowlist.
    Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| ApiError::internal())
}

async fn bounded_body(mut response: Response) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BODY as u64)
    {
        return Err(ApiError::unavailable(
            "Merchant response exceeds the size limit.",
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ApiError::unavailable("Merchant response unavailable."))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_BODY {
            return Err(ApiError::unavailable(
                "Merchant response exceeds the size limit.",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    const PAYER: &str = "0x19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A";
    const SIG:&str="0x51f9583721a6c2a55d00bd992d791c555fe43699a56e9a4186f85ea0d48090c17bee947e2a26701e64bd3ca5887c91432b973c79ad2540f8580a4b3f322a765a1c";
    fn merchant() -> Merchant {
        Merchant {
            id: "reports".into(),
            resource_url: "https://reports.example/task".into(),
            facilitator_url: "https://pay.example".into(),
            recipient: "0x2222222222222222222222222222222222222222".into(),
            fee_payer: "0x3333333333333333333333333333333333333333".into(),
            network: NETWORK.into(),
            asset: USDT.into(),
            max_amount: 50_000_000_000_000_000_000,
        }
    }
    fn limits() -> Limits {
        Limits {
            per_call: 50_000_000_000_000_000_000,
            daily_remaining: 60_000_000_000_000_000_000,
            total_remaining: 60_000_000_000_000_000_000,
        }
    }
    fn required() -> Value {
        let m = merchant();
        json!({"x402Version":2,"resource":{"url":m.resource_url},"accepts":[{"scheme":"exact","network":NETWORK,"asset":USDT,"payTo":m.recipient,"amount":"30000000000000000001","maxTimeoutSeconds":60,"extra":{"assetTransferMethod":"permit2"}}]})
    }
    fn header(v: &Value) -> String {
        STANDARD.encode(serde_json::to_vec(v).unwrap())
    }
    fn offer() -> Offer {
        validate_offer(&header(&required()), &merchant(), limits()).unwrap()
    }
    fn auth() -> Authorization {
        let mut a = Authorization::new(&offer(), PAYER, 1000).unwrap();
        a.nonce = "123456".into();
        a
    }
    #[test]
    fn preserves_18_decimals_and_binds_offer() {
        assert_eq!(offer().amount_units, 30_000_000_000_000_000_001);
        assert_eq!(
            serde_json::to_value(offer()).unwrap()["amount_units"],
            "30000000000000000001"
        );
        for field in ["network", "asset", "payTo", "scheme"] {
            let mut r = required();
            r["accepts"][0][field] = json!("wrong");
            assert!(validate_offer(&header(&r), &merchant(), limits()).is_err());
        }
        let mut r = required();
        r["accepts"][0]["extra"]["assetTransferMethod"] = json!("eip3009");
        assert!(validate_offer(&header(&r), &merchant(), limits()).is_err());
        r["accepts"][0]["extra"] = json!({});
        assert!(validate_offer(&header(&r), &merchant(), limits()).is_err());
    }
    #[test]
    fn facilitator_discovery_accepts_optional_metadata_and_evm_wildcard_signers() {
        let m = merchant();
        let mut supported = json!({"kinds":[{"x402Version":2,"scheme":"exact","network":NETWORK}],"signers":{"eip155:*":[m.fee_payer]}});
        assert!(supports_merchant(&supported, &m));
        supported["signers"] = json!({NETWORK:[m.fee_payer]});
        assert!(supports_merchant(&supported, &m));
        supported["kinds"][0]["extra"] = json!({"assetTransferMethod":"permit2"});
        assert!(supports_merchant(&supported, &m));
        for field in ["scheme", "network", "x402Version"] {
            let mut changed = supported.clone();
            changed["kinds"][0][field] = json!("wrong");
            assert!(!supports_merchant(&changed, &m));
        }
        for method in [json!("eip3009"), json!("permit2-exact"), Value::Null] {
            let mut changed = supported.clone();
            changed["kinds"][0]["extra"]["assetTransferMethod"] = method;
            assert!(!supports_merchant(&changed, &m));
        }
        for signers in [
            json!({"eip155:* ":[m.fee_payer]}),
            json!({"eip155:8453":[m.fee_payer]}),
            json!({"*":[m.fee_payer]}),
            json!({"eip155:*":[m.recipient]}),
        ] {
            let mut changed = supported.clone();
            changed["signers"] = signers;
            assert!(!supports_merchant(&changed, &m));
        }
    }
    #[test]
    fn merchant_timeout_is_a_maximum_and_local_authorizations_stay_bounded() {
        for timeout in [1, 60, 120, 300, u64::MAX] {
            let mut required = required();
            required["accepts"][0]["maxTimeoutSeconds"] = json!(timeout);
            let offer = validate_offer(&header(&required), &merchant(), limits()).unwrap();
            let auth = Authorization::new(&offer, PAYER, 1000).unwrap();
            assert_eq!(auth.deadline, (1000 + timeout.min(120)).to_string());
            let mut extended = auth.clone();
            extended.deadline = (1001 + timeout.min(120)).to_string();
            assert!(extended.validate(&offer, PAYER, 1000).is_err());
            let clipped = Authorization::new(&offer, PAYER, 86_390).unwrap();
            assert_eq!(clipped.deadline, (86_390 + timeout.min(9)).to_string());
            assert!(Authorization::new(&offer, PAYER, 86_399).is_err());
        }
        let mut required = required();
        required["accepts"][0]["maxTimeoutSeconds"] = json!(0);
        assert!(validate_offer(&header(&required), &merchant(), limits()).is_err());
    }
    #[test]
    fn installed_dexscreener_merchant_matches_observed_payment_requirements() {
        let merchants: Vec<Merchant> =
            serde_json::from_str(include_str!("../config/x402-merchants-bnb.json")).unwrap();
        assert_eq!(merchants.len(), 1);
        let m = &merchants[0];
        m.validate(NETWORK, USDT).unwrap();
        assert_eq!(m.max_amount, 1_000_000_000_000);
        // Public 402 response observed 2026-10-06; no authorization was sent.
        let required = json!({"x402Version":2,"resource":{"url":"https://x402-gateway.bankofai.io/providers/dexscreener-dex-data-bsc/latest/dex/tokens/0x55d398326f99059fF775485246999027B3197955"},"accepts":[{"scheme":"exact","network":"eip155:56","amount":"1000000000000","asset":"0x55d398326f99059fF775485246999027B3197955","payTo":"0x7bac3352Bc5F342DcaFA573749aA4502CB12dA86","maxTimeoutSeconds":300,"extra":{"assetTransferMethod":"permit2"}}]});
        let offer = validate_offer(&header(&required), m, limits()).unwrap();
        assert_eq!(offer.amount_units, 1_000_000_000_000);
        assert_eq!(
            Authorization::new(&offer, PAYER, 1000).unwrap().deadline,
            "1120"
        );
        let mut too_expensive = required;
        too_expensive["accepts"][0]["amount"] = json!("1000000000001");
        assert!(validate_offer(&header(&too_expensive), m, limits()).is_err());
    }
    #[tokio::test]
    #[ignore = "read-only live merchant discovery; requires public network access"]
    async fn live_dexscreener_discovery_and_quote_require_no_signature() {
        let merchants: Vec<Merchant> =
            serde_json::from_str(include_str!("../config/x402-merchants-bnb.json")).unwrap();
        let m = &merchants[0];
        assert!(sponsor_supported(m).await.unwrap());
        let offer = fetch_offer(m, NETWORK, USDT, limits()).await.unwrap();
        assert_eq!(offer.amount_units, m.max_amount);
        assert_eq!(offer.accepted.extra["assetTransferMethod"], "permit2");
        assert_eq!(offer.accepted.network, NETWORK);
    }
    #[test]
    fn enforces_all_budgets_and_canonical_amounts() {
        for l in [
            Limits {
                per_call: 1,
                ..limits()
            },
            Limits {
                daily_remaining: 1,
                ..limits()
            },
            Limits {
                total_remaining: 1,
                ..limits()
            },
        ] {
            assert!(validate_offer(&header(&required()), &merchant(), l).is_err());
        }
        for n in [
            "-1",
            "+1",
            "01",
            "1.0",
            "0",
            "340282366920938463463374607431768211456",
        ] {
            let mut r = required();
            r["accepts"][0]["amount"] = json!(n);
            assert!(validate_offer(&header(&r), &merchant(), limits()).is_err());
        }
    }
    #[test]
    fn matches_independent_viem_signature_vector() {
        let a = auth();
        assert_eq!(
            hex::encode(a.digest().unwrap()),
            "2121fd5521fa16f9d44114a7d058b09fb89495f15fb971f1330c8b4d3a94113e"
        );
        a.verify_eoa_signature(SIG).unwrap();
        assert!(payment_signature(&offer(), &a, SIG, 1000).is_ok());
        let mut changed = a.clone();
        changed.witness.to = merchant().fee_payer;
        assert!(changed.verify_eoa_signature(SIG).is_err());
        changed = a.clone();
        changed.nonce = "123457".into();
        assert!(changed.verify_eoa_signature(SIG).is_err());
        assert!(a.validate(&offer(), PAYER, 1060).is_err());
        assert!(a.validate(&offer(), &merchant().recipient, 1000).is_err());
    }
    fn transaction() -> (Value, Value, String) {
        let a = auth();
        let m = merchant();
        let h = format!("0x{}", "ab".repeat(32));
        let receipt = json!({"status":"0x1","transactionHash":h,"logs":[{"address":USDT,"removed":false,"topics":[format!("0x{}",hex::encode(keccak("Transfer(address,address,uint256)"))),format!("0x{:0>64}",&a.from[2..]),format!("0x{:0>64}",&m.recipient[2..])],"data":format!("0x{:064x}",U256::from_dec_str(&a.permitted.amount).unwrap())}]});
        let tx = json!({"hash":h,"chainId":"0x38","from":m.fee_payer,"to":EXACT_PROXY,"input":a.settlement_calldata(SIG).unwrap()});
        (receipt, tx, h)
    }
    #[test]
    fn rejects_wrong_receipt_nonce_token_and_duplicate_transfer() {
        let (r, t, h) = transaction();
        verify_transfer(&r, &t, &h, &auth(), SIG, &merchant()).unwrap();
        for field in ["chainId", "input", "to", "from", "hash"] {
            let mut bad = t.clone();
            bad[field] = json!("wrong");
            assert!(verify_transfer(&r, &bad, &h, &auth(), SIG, &merchant()).is_err());
        }
        for field in ["address", "data"] {
            let mut bad = r.clone();
            bad["logs"][0][field] = json!("wrong");
            assert!(verify_transfer(&bad, &t, &h, &auth(), SIG, &merchant()).is_err());
        }
        let mut bad = r.clone();
        let duplicate = bad["logs"][0].clone();
        bad["logs"].as_array_mut().unwrap().push(duplicate);
        assert!(verify_transfer(&bad, &t, &h, &auth(), SIG, &merchant()).is_err());
        let mut bad = r;
        bad["status"] = json!("0x0");
        assert!(verify_transfer(&bad, &t, &h, &auth(), SIG, &merchant()).is_err());
    }
    #[test]
    fn settlement_payer_is_token_owner_not_facilitator() {
        let m = merchant();
        let r = json!({"success":true,"transaction":format!("0x{}","ab".repeat(32)),"network":NETWORK,"payer":PAYER});
        assert_eq!(settlement_response(&header(&r), &m).unwrap().payer, PAYER);
    }
    #[test]
    fn rejects_untrusted_endpoint_configuration() {
        for url in [
            "http://example.com",
            "https://localhost/a",
            "https://127.0.0.1/a",
            "https://169.254.169.254/a",
            "https://[::1]/a",
            "https://user:password@example.com/a",
            "https://example.com/a#b",
        ] {
            assert!(public_https(url).is_err(), "{url}");
        }
    }
    #[test]
    fn permit_deadline_stays_in_reserved_utc_day() {
        let a = Authorization::new(&offer(), PAYER, 86_390).unwrap();
        assert_eq!(a.deadline, "86399");
        assert!(a.validate(&offer(), PAYER, 86_400).is_err());
    }
    #[tokio::test]
    async fn failed_service_body_retains_settlement_hash() {
        let response = reqwest::Response::from(
            axum::http::Response::builder()
                .status(200)
                .header("content-length", MAX_BODY + 1)
                .body("x".repeat(MAX_BODY + 1))
                .unwrap(),
        );
        let settlement = Settlement {
            success: true,
            transaction: format!("0x{}", "ab".repeat(32)),
            network: NETWORK.into(),
            payer: PAYER.into(),
        };
        let (_, receipt, delivered) = paid_body(response, settlement).await;
        assert!(!delivered);
        assert_eq!(receipt.transaction, format!("0x{}", "ab".repeat(32)));
    }
}
