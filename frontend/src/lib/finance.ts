import { decodeFunctionData, erc20Abi, isAddress, parseAbi, type Abi, type Hex } from "viem";
import { isRecord } from "./api";
import type { JobActionRecord, RuntimeAgent, WalletActionIntent } from "./api";
import { usdtUnits } from "./amounts";
import { BNB_CHAIN_ID, USDT_ADDRESS, sameAddress, validateTransaction } from "./evm";

export type FinanceModule = { status: "not_deployed" | "verified" | "unavailable"; address: string | null; reason: string; paused?: boolean; available_usdt?: string; underwriter?: string };
export type FinanceAsset = { token: string; symbol: string; decimals: number; feed: string; ltv_bps: number; liquidation_bps: number; max_age: number; market_open: boolean; status: string; price_usdt: string | null };
export type FinanceSystem = { chain_id: 56; currency: "USDT"; source: "verified_onchain" | "deployment_pending"; modules: {
  lending: FinanceModule & { available_usdt: string; total_assets_usdt: string; outstanding_usdt: string; total_shares: string };
  stock_loans: FinanceModule & { assets: FinanceAsset[]; total_assets_usdt?: string; outstanding_usdt?: string; total_shares?: string };
  buyback: FinanceModule & { official_token: string | null; spent_usdt: string; tokens_burned: string; operator?: string; token_decimals?: number };
} };
export type AdvanceRequest = { id: string; loan_id: string; agent_id: string; agent_name: string; job_id: string; job_title: string; borrower: string; amount: string; per_call: string; daily_cap: string; expires_at: number; signer: string; tools: string[]; recipients: string[]; status: string; created_at: string; public: boolean };
export type AdvanceLoan = { id: string; agent: string; job: string; borrower: string; signer: string; principal: string; available: string; debt: string; loss: string; spent: string; repaid: string; per_call: string; daily_cap: string; expires_at: number; tools: string[]; recipients: string[]; accepted: boolean; closed: boolean; actions: { accept: boolean; spend: boolean; repay: boolean; close: boolean } };
export type StockLoan = { id: string; borrower: string; token: string; symbol: string; decimals: number; collateral: string; debt: string; loss: string; collateral_value_usdt: string | null; maximum_borrow_usdt: string | null; liquidation_debt_usdt: string | null; liquidatable: boolean; oracle_status: string; actions: { add_collateral: boolean; withdraw: boolean; repay: boolean; liquidate: boolean } };
export type FinancePosition = { shares: string; max_redeem: string; share_value_usdt: string };
export type FinanceData = { system: FinanceSystem; wallet: string; status: string; lending: FinancePosition & { loans: AdvanceLoan[] }; stock_loans: FinancePosition & { loans: StockLoan[] }; requests: AdvanceRequest[]; pending_intents: WalletActionIntent[]; pending_job_intents?: JobActionRecord[]; roles: { underwriter: boolean; buyback_operator: boolean } };
export const financeActions = ["pool_deposit", "pool_redeem", "stock_deposit", "stock_redeem", "advance_approve", "advance_accept", "advance_spend", "advance_repay", "advance_close", "stock_borrow", "stock_add_collateral", "stock_withdraw", "stock_repay", "stock_liquidate", "buyback_fund", "buyback_execute"] as const;
export type FinanceAction = typeof financeActions[number];
export type FinanceInput = { action: FinanceAction | "advance_request"; amount: string; loan_id?: string; request_id?: string; job_id?: string; target_agent_id?: string; token_address?: string; collateral_amount?: string; per_call?: string; daily_cap?: string; expires_at?: number; signer?: string; tools?: string[]; recipients?: string[]; recipient?: string; tool?: string; request_hash?: string; receipt_hash?: string; minimum_out?: string; deadline?: number; public?: boolean };
export type FinanceQuote = { status: string; action: string; chain_id: number; currency: string; reason?: string; token?: string; decimals?: number; collateral_value_usdt?: string; maximum_borrow_usdt?: string; liquidation_debt_usdt?: string; price_usdt?: string; maximum_repay_usdt?: string; collateral_out?: string; quoted_tokens?: string; minimum_tokens?: string; deadline?: number };
export const financeAction = (action: string) => action.replace(/^finance_/, "");
export const isFinanceIntent = (intent: WalletActionIntent) => intent.action.startsWith("finance_") && financeActions.includes(financeAction(intent.action) as FinanceAction);
export const financeModuleKey = (action: string): "lending" | "stock_loans" | "buyback" => action.startsWith("stock_") ? "stock_loans" : action.startsWith("buyback_") ? "buyback" : "lending";
export function financeUnits(value: string, decimals = 18): bigint {
  const units = usdtUnits(value);
  if (units === null || !Number.isInteger(decimals) || decimals < 0 || decimals > 18 || units % 10n ** BigInt(18 - decimals) !== 0n) throw Error("Enter an exact amount within the token's decimal precision.");
  return units / 10n ** BigInt(18 - decimals);
}
export const lendingABI = parseAbi([
  "function deposit(uint256 assets,address receiver)", "function redeem(uint256 shares,address receiver,address owner)",
  "function approveLoan((bytes32 id,bytes32 agent,bytes32 job,address signer,uint256 principal,uint256 perCall,uint256 dailyCap,uint64 expiresAt,uint64 tools,address[] recipients) t)",
  "function acceptLoan(bytes32 id)", "function spendLoan(bytes32 id,address recipient,uint256 amount,uint64 tool,bytes32 request,bytes32 receipt)", "function repayLoan(bytes32 id,uint256 amount)", "function closeLoan(bytes32 id)",
]);
export const stockABI = parseAbi(["function deposit(uint256 assets,address receiver)", "function redeem(uint256 shares,address receiver,address owner)", "function borrow(bytes32 id,address token,uint256 collateralAmount,uint256 principal)", "function addCollateral(bytes32 id,uint256 amount)", "function withdrawCollateral(bytes32 id,uint256 amount)", "function repay(bytes32 id,uint256 amount)", "function liquidate(bytes32 id,uint256 principal,uint256 minimumCollateral)"]);
export const buybackABI = parseAbi(["function fund(uint256 amount)", "function execute(uint256 amount,uint256 minimumTokens,uint256 deadline)"]);
const tools = ["bnb-rpc", "openrouter", "tavily", "web-search", "x402"];
const toolMask = (selected: string[]) => selected.reduce((bits, tool) => { const index = tools.indexOf(tool); if (index < 0) throw Error("This action contains an unknown tool."); return bits | 1n << BigInt(index); }, 0n);
const hash = (value: unknown): value is Hex => typeof value === "string" && /^0x[0-9a-fA-F]{64}$/.test(value) && !/^0x0{64}$/.test(value);
const equal = (a: unknown, b: unknown) => typeof a === "bigint" || typeof b === "bigint" ? a === b : typeof a === "string" && typeof b === "string" && /^0x/.test(a) && /^0x/.test(b) ? a.toLowerCase() === b.toLowerCase() : JSON.stringify(a) === JSON.stringify(b);

/** Decode every financial call and bind its destinations and token approvals to the reviewed terms. */
export function validateFinanceIntent(intent: WalletActionIntent, agent: RuntimeAgent, data: FinanceData, expected?: FinanceInput, quote?: FinanceQuote) {
  const action = financeAction(intent.action) as FinanceAction;
  const module = data.system.modules[financeModuleKey(action)];
  if (!isFinanceIntent(intent) || data.system.chain_id !== BNB_CHAIN_ID || data.system.currency !== "USDT" || !sameAddress(data.wallet, agent.wallet) || intent.chain_id !== BNB_CHAIN_ID || intent.agent_id !== agent.id || !sameAddress(intent.sender, agent.wallet)) throw Error("Finance preparation belongs to another action, agent, wallet or network.");
  if (module.status !== "verified" || !isAddress(module.address || "", { strict: false }) || !sameAddress(intent.to, module.address)) throw Error("The finance contract destination has not been verified.");
  if (!Number.isFinite(Date.parse(intent.expires_at)) || Date.parse(intent.expires_at) <= Date.now()) throw Error("This unsigned finance action expired. Refresh its terms before signing.");
  const details = intent.details;
  if (!isRecord(details) || details.action !== action) throw Error("The reviewed finance action does not match its payload.");
  if (expected) for (const [key, value] of Object.entries(expected)) {
    if (["amount", "collateral_amount", "minimum_out", "per_call", "daily_cap"].includes(key) && typeof value === "string" && typeof details[key] === "string") { if (usdtUnits(value) !== usdtUnits(details[key])) throw Error("The prepared finance amount changed. Review the terms again."); }
    else if (!equal(details[key], value)) throw Error("The prepared finance terms changed. Review the terms again.");
  }
  if (!intent.transactions.length || intent.transactions.length > 3) throw Error("Unexpected finance transaction sequence.");
  intent.transactions.forEach(validateTransaction);
  const final = intent.transactions.at(-1)!;
  if (!sameAddress(final.to, module.address) || !sameAddress(final.to, intent.to) || final.data.toLowerCase() !== intent.data.toLowerCase() || BigInt(final.value) !== 0n || BigInt(intent.value) !== 0n || !sameAddress(intent.transaction.to, final.to) || intent.transaction.data.toLowerCase() !== final.data.toLowerCase() || BigInt(intent.transaction.value) !== 0n) throw Error("The finance transaction differs from its reviewed summary.");
  const abi: Abi = financeModuleKey(action) === "lending" ? lendingABI : financeModuleKey(action) === "stock_loans" ? stockABI : buybackABI;
  const call = decodeFunctionData({ abi, data: final.data as Hex });
  const args = (call.args || []) as readonly unknown[];
  const amount = financeUnits(typeof details.amount === "string" ? details.amount : "0");
  if (!["advance_approve", "advance_accept", "advance_close", "stock_add_collateral", "stock_withdraw"].includes(action) && amount <= 0n) throw Error("The finance transfer amount must be positive.");
  const loanId = details.loan_id;
  const loan = data.stock_loans.loans.find(item => item.id === loanId);
  if (loan && details.token_address && !sameAddress(String(details.token_address), loan.token)) throw Error("The collateral token differs from this position.");
  if (quote && (quote.chain_id !== BNB_CHAIN_ID || quote.currency !== "USDT" || quote.action !== action || quote.status !== "verified")) throw Error("The reviewed finance quote belongs to another action or network.");
  const quotedCollateral = action === "stock_liquidate" && quote?.token && quote.decimals !== undefined ? { token: quote.token, decimals: quote.decimals } : null;
  if (quotedCollateral && (!sameAddress(String(details.token_address), quotedCollateral.token) || Number(details.token_decimals) !== quotedCollateral.decimals)) throw Error("The liquidation asset differs from the verified quote.");
  const configuredAsset = data.system.modules.stock_loans.assets.find(item => sameAddress(item.token, String(details.token_address)));
  const savedAsset = loan ? { token: loan.token, decimals: loan.decimals } : null;
  const asset = action === "stock_borrow" ? configuredAsset : quotedCollateral || savedAsset || configuredAsset;
  const decimals = asset?.decimals;
  let approvalToken: string | null = null;
  let approvalAmount = 0n;
  const requireCall = (name: string, values: unknown[]) => { if (call.functionName !== name || args.length !== values.length || args.some((value, index) => !equal(value, values[index]) && !(typeof value === "bigint" && typeof values[index] === "bigint" && value === values[index]))) throw Error("The finance calldata differs from the reviewed amounts, recipient or loan."); };
  const requireLoan = () => { if (!hash(loanId)) throw Error("The loan identifier is invalid."); return loanId; };
  if (action === "pool_deposit" || action === "stock_deposit") { requireCall("deposit", [amount, agent.wallet]); approvalToken = USDT_ADDRESS; approvalAmount = amount; }
  else if (action === "pool_redeem" || action === "stock_redeem") requireCall("redeem", [amount, agent.wallet, agent.wallet]);
  else if (action === "advance_approve") {
    const request = data.requests.find(item => item.id === details.request_id);
    if (!data.roles.underwriter || !sameAddress(module.underwriter, agent.wallet) || !request || !hash(details.registry_id) || !hash(details.job_onchain_id) || !hash(request.loan_id)) throw Error("The underwriter or approved request terms cannot be verified.");
    const localAgentId = request.agent_id.replace(/^0x/, "");
    const localJobId = request.job_id.replace(/^0x/, "");
    if (!isAddress(request.borrower, { strict: false }) || !/^[0-9a-fA-F]{32}$/.test(localAgentId) || !/^(?:[0-9a-fA-F]{32}|[0-9a-fA-F]{64})$/.test(localJobId)) throw Error("The reviewed request's agent and job identifiers are invalid.");
    const registryId = `${request.borrower.toLowerCase()}${localAgentId.slice(8).toLowerCase()}`;
    const jobId = `0x${localJobId.toLowerCase().padStart(64, "0")}`;
    if (!equal(details.registry_id, registryId) || !equal(details.job_onchain_id, jobId)) throw Error("The advance targets a different borrower agent or job.");
    const terms = { id: request.loan_id, agent: registryId, job: jobId, signer: request.signer, principal: financeUnits(request.amount), perCall: financeUnits(request.per_call), dailyCap: financeUnits(request.daily_cap), expiresAt: BigInt(request.expires_at), tools: toolMask(request.tools), recipients: request.recipients };
    if (call.functionName !== "approveLoan" || !isRecord(args[0]) || Object.entries(terms).some(([key, value]) => {
      const actual = (args[0] as Record<string, unknown>)[key];
      return typeof value === "bigint" ? actual !== value : Array.isArray(value) ? !Array.isArray(actual) || value.length !== actual.length || value.some((address, i) => !sameAddress(address, String(actual[i]))) : !equal(actual, value);
    })) throw Error("The onchain advance differs from the request you reviewed.");
  } else if (action === "advance_accept") requireCall("acceptLoan", [requireLoan()]);
  else if (action === "advance_close") requireCall("closeLoan", [requireLoan()]);
  else if (action === "advance_repay") { requireCall("repayLoan", [requireLoan(), amount]); approvalToken = USDT_ADDRESS; approvalAmount = amount; }
  else if (action === "advance_spend") { if (!isAddress(String(details.recipient), { strict: false }) || typeof details.tool !== "string" || !hash(details.request_hash) || !hash(details.receipt_hash)) throw Error("The approved payment receipt terms are invalid."); requireCall("spendLoan", [requireLoan(), details.recipient, amount, toolMask([details.tool]), details.request_hash, details.receipt_hash]); }
  else if (action === "stock_borrow") { if (!asset || decimals === undefined || typeof details.collateral_amount !== "string") throw Error("The approved collateral token is unavailable."); const collateral = financeUnits(details.collateral_amount, decimals); requireCall("borrow", [requireLoan(), asset.token, collateral, amount]); approvalToken = asset.token; approvalAmount = collateral; }
  else if (action === "stock_add_collateral" || action === "stock_withdraw") { if (!asset || decimals === undefined || typeof details.collateral_amount !== "string") throw Error("The position's collateral precision is unavailable."); const collateral = financeUnits(details.collateral_amount, decimals); requireCall(action === "stock_add_collateral" ? "addCollateral" : "withdrawCollateral", [requireLoan(), collateral]); if (action === "stock_add_collateral") { approvalToken = asset.token; approvalAmount = collateral; } }
  else if (action === "stock_repay") { requireCall("repay", [requireLoan(), amount]); approvalToken = USDT_ADDRESS; approvalAmount = amount; }
  else if (action === "stock_liquidate") { if (decimals === undefined || typeof details.minimum_out !== "string") throw Error("The liquidation collateral minimum is unavailable."); requireCall("liquidate", [requireLoan(), amount, financeUnits(details.minimum_out, decimals)]); approvalToken = USDT_ADDRESS; approvalAmount = amount; }
  else if (action === "buyback_fund") { requireCall("fund", [amount]); approvalToken = USDT_ADDRESS; approvalAmount = amount; }
  else if (action === "buyback_execute") { const buyback = data.system.modules.buyback; const tokenDecimals = buyback.token_decimals ?? Number(details.token_decimals); if (!data.roles.buyback_operator || !sameAddress(buyback.operator, agent.wallet) || !isAddress(buyback.official_token || "", { strict: false }) || typeof details.minimum_out !== "string" || typeof details.deadline !== "number" || details.deadline <= Date.now() / 1000 || details.deadline > Date.now() / 1000 + 600) throw Error("The buyback operator, token or deadline is invalid."); requireCall("execute", [amount, financeUnits(details.minimum_out, tokenDecimals), BigInt(details.deadline)]); }
  else throw Error("Unsupported finance action.");
  const approvals = intent.transactions.slice(0, -1);
  if (!approvalToken && approvals.length) throw Error("This finance action does not need a token approval.");
  approvals.forEach((transaction, index) => {
    if (!sameAddress(transaction.to, approvalToken) || BigInt(transaction.value) !== 0n) throw Error("The finance token approval targets another asset.");
    const decoded = decodeFunctionData({ abi: erc20Abi, data: transaction.data as Hex });
    if (decoded.functionName !== "approve" || !sameAddress(String(decoded.args[0]), module.address) || (decoded.args[1] !== approvalAmount && !(decoded.args[1] === 0n && index === 0 && approvals.length === 2))) throw Error("Approve only the exact reviewed token amount to this finance contract.");
  });
}
