# Tab on BNB Smart Chain

The active contracts are non-upgradeable Solidity contracts for BNB Smart Chain (chain 56). BNB pays gas. Jobs, execution budgets, outcome pools and lending use 18-decimal Binance-Peg USDT at `0x55d398326f99059fF775485246999027B3197955`. The constructor rejects other production chains/tokens; chain 31337 permits local test tokens.

The active `TabProtocol`, `TabBacking`, `TabEconomics` and shared `TabTypes` sources implement the v2 deployment recorded in [the current manifest](../deployments/bnb-56.json). The protocol charges 0.5% on accepted work rewards, with a zero fee for executors holding the configured official TAB token at settlement. The backing contract supports collateralized, zero-interest credit and liquidation. The configured official TAB token is `0xf07449517ae4b48808098c573a5347e67c714444` on BNB (18 decimals). Its fixed proxy implementation and code hash are pinned in the active manifest.

The previous 2% unsecured deployment is retained in [the legacy manifest](../deployments/bnb-56-legacy.json). Historical experiments are preserved in [future/unsupported-legacy-credit](future/unsupported-legacy-credit/README.md), outside the active source and test directories. Old addresses and experimental ABIs cannot substitute for the active deployment. Verify runtime hashes and module wiring against the selected manifest before preparing transactions.

## Build and test

Requires Node/npm and Foundry. Dependencies are pinned to OpenZeppelin 5.0.2, forge-std 1.16.2, and Solidity 0.8.28; Paris opcodes preserve BNB compatibility.

```sh
cd contracts/bnb
npm ci --ignore-scripts
git clone --branch v1.16.2 --depth 1 https://github.com/foundry-rs/forge-std.git lib/forge-std
forge build --sizes
forge test --summary
forge fmt --check
BNB_FORK_RPC_URL=https://bsc-dataseed.bnbchain.org forge test --match-contract TabBNBForkTest -vv
```

The forge-std v1.16.2 tag resolves to `bf647bd6046f2f7da30d0c2bf435e5c76a780c1b`. The tests exercise signatures, owner namespaces, scoped sessions, replay protection, shared daily spending limits, job tree budget reservations, refunds, actual lender principal, token decimals, unsafe transfer behavior, native BNB reentrancy, staking, bonds, bounties, outcomes and rounding conservation. Stateful invariants combine these payment paths and compare every recorded liability with token balances. Fuzz tests run 1,024 cases; each invariant runs 128 sequences of up to 64 calls.

Regenerate checked-in ABIs after changing public interfaces:

```sh
python3 - <<'PY'
import json
from pathlib import Path
for name in ['TabProtocol', 'TabBacking', 'TabEconomics', 'TabAgentToken']:
    source = 'TabEconomics' if name == 'TabAgentToken' else name
    artifact = json.loads(Path('out', source + '.sol', name + '.json').read_text())
    Path('abi', name + '.json').write_text(json.dumps(artifact['abi'], indent=2) + '\n')
PY
```

## Deployment and authority

1. Deploy `TabProtocol(usdtAddress, authority)`.
2. Deploy `TabBacking(protocolAddress)` and `TabEconomics(protocolAddress)`.
3. The authority calls `TabProtocol.configureModules(backing, economics)` once. Each module must report the same protocol address.
4. Only if an existing official BNB TAB contract is independently verified, call `configureTab(token)` once. This action records that token; it never creates an official replacement.
5. Record addresses, chain ID, USDT decimals and deployed code hashes. Verify the token, authority and module getters against that manifest before enabling writes.

There are no proxy upgrades or administrative withdrawals. Protocol module wiring and official-token configuration are one-time authority actions. The backing contract also lets the authority configure an additional collateral token once, with immutable risk parameters, and pause or resume new debt against configured collateral. Those permissions do not grant custody withdrawals. USDT approvals are separate for each custody contract and should match the exact requested amount. Deployment alone does not fund budgets, lend principal or launch user coins.

## State and accounting

`TabProtocol` owns agent registration, direct and sponsored EIP-712 registration, bounded session payments, owned USDT budgets and delegated job escrows. Agent IDs contain the owner's address as their first 20 bytes and a unique 12-byte suffix. Registration verifies this namespace. Sponsored signatures include all terms, nonce, deadline, chain ID and verifying contract. Session grants are immutable and revocable; provider payments require a unique request hash and receipt hash, an allowed recipient/tool, unexpired limits, sufficient funds and the agent-wide daily cap.

The buyer fixes the allowed provider recipients in the funded job terms. An empty recipient list disables provider payouts. A provider payment cannot go to the executor or buyer. Each child job reserves part of its parent's available budget. It cannot widen the parent's recipients, tools, maximum call or deadline; delegation stops at depth eight. Parents cannot settle with open descendants. The original root buyer approves or rejects every reward release at every depth. A delegated executor cannot pay a child reward by approving their own child branch. Submission remains possible while paused. A permanent timely-submission fact prevents a buyer from converting rejected evidence into a missed-deadline bond penalty. Accepted rewards pay 0.5%, reduced to zero when the executor holds the configured official TAB token at settlement. Fees stay in custody as a reserve. The core protocol has no swap, automatic liquidity or token purchase path.

`TabBacking` records exact units of BEP-20 tokens and native BNB per agent and backer. Token decimals are read from the actual token, including stock tokens; fee-on-transfer deposits/withdrawals are rejected. The contract does not classify tokens as verified stocks, attach a dollar price, grant lending power against them or spend them. Issuer transfer restrictions, freezing, rebases or upgrade powers can affect the ability to withdraw the issuer's tokens.

USDT credit is separately and actually funded by a lender. The lender fixes the agent, allowed signer, recipients, tools, expiration and per-call/daily limits. The borrower explicitly accepts and pledges the agreed collateral before spending. New debt must fit the collateral's borrowing limit, asset debt cap and available lender funds. Funds can go only to approved recipients; they cannot be withdrawn by the borrower. The borrower incurs principal debt when a provider payment settles. Anyone may repay outstanding principal. The lender may withdraw only unspent or repaid units. Either party can close further spending without blocking repayment. Credit charges no interest and promises no yield or principal protection. Ordinary backing deposits remain independent custody until a separate, explicit pledge is made. Lender conservation is `funded = available + outstanding + withdrawn`.

USDT collateral is configured at construction with a 90% borrowing limit, 95% liquidation threshold and 2% liquidation bonus, measured directly in the same debt token. Additional assets require a token-specific oracle and immutable risk configuration. Liquidation is available when debt exceeds the liquidation threshold or remains unpaid after expiry plus a 24-hour grace period. A liquidator pays actual USDT and receives the quoted collateral, subject to a minimum-output bound. Oracle failure prevents fresh valuation-dependent actions; repayment and fully repaid collateral withdrawal remain available. Stock custody alone supplies no borrowing power.

`TabEconomics` can deploy a fixed-supply agent BEP-20 coin with no further minting, pause or tax, or record an existing coin. Existing-token pairing certifies identity only, not issuer permissions. It supports time-locked custody of a verified official TAB token, objective missed-deadline bonds using each agent's paired coin, and USDT pari-mutuel outcomes. Bonds never promise dollar reimbursement. Outcome pools measure only whether the executor posted evidence before the job deadline, not work quality. One-sided pools and qualifying early cancellations refund contributors; winner payouts conserve rounding dust. TAB holder-gated public bounty claims are in `TabProtocol`.

Official TAB configuration enables staking, holder-gated bounty claims and outcomes. Bounty claims and outcome participation also require the relevant agent token. Staking locks tokens for 1–31 days and pays no rewards; only TAB still in the wallet counts for app access and the fee exemption. The app requires TAB holdings for new actions, while deployed core registration, budgets, jobs and backing retain their existing onchain permissions. The archived chain implementation is outside the active repository tree.

## Additional finance modules

`TabLendingPool`, `TabStockLending`, `TabMarketHours` and `TabBuyback` are separate, non-upgradeable release candidates. They do not modify the deployed protocol, backing or economics addresses. Their proposed production addresses and code hashes remain `null` in `backend/config/finance-bnb.json`; no liquidity, collateral, loan or buyback is created by deploying them. The API must verify each module's protocol, USDT, authority, bytecode and configured policy before presenting a wallet transaction.

`TabLendingPool` is an ERC4626 USDT pool with principal-only, zero-interest job advances. An underwriter approves a specific funded job that is bound to an agent. The agent owner accepts the line; its signer can pay only recipients and tools already allowed by both the job and loan. Per-call, daily, expiration and replay limits apply. Unspent approvals reserve liquidity until the borrower/underwriter closes them or anyone closes an expired/settled line. Repayment reduces debt without reopening the line. Lender redemptions are limited to unreserved cash; a loan receivable is not available liquidity. No interest, APR or principal protection is promised. After expiry plus seven days, the underwriter can publicly recognize unpaid principal as a loss without forgiving the borrower's debt. Recognized loss reduces share value; later repayment benefits the pool's shareholders at recovery time.

The pool enforces its own agent-wide daily cap and includes protocol spending already visible when a pool payment is made. The existing immutable protocol cannot consume or see this new module's daily counter, so subsequent legacy session/job/credit payments cannot share one atomic global cap with the new pool. Keep the two spending scopes distinct in the UI. The existing escrow still pays its executor normally; working-capital repayment is an explicit wallet transaction, not an automatic deduction or assignment of job proceeds. An advance remains unsecured even when its job is funded.

`TabStockLending` uses a separate ERC4626 USDT pool. A collateral asset is disabled until the underwriter configures its exact BEP20 token, collateral/USD AggregatorV3 feed, market-status contract, maximum feed age, borrowing LTV, liquidation LTV, liquidation bonus and principal cap. A separate, fresh USDT/USD feed converts collateral USD value into actual USDT units; the model does not assume a fixed USDT peg. Token/feed decimals, positive answers, completed rounds and timestamps are checked. Every loan snapshots the collateral risk terms, so changing the whitelist does not retroactively change a borrower's LTV or oracle route.

Borrowing and active-debt collateral withdrawals require current prices and an open market. Repayment and collateral top-ups remain possible while paused, closed or stale; a fully repaid borrower can recover collateral without an oracle. A liquidator repays exact USDT for quoted collateral plus the configured bonus and supplies a minimum collateral amount. Liquidation also requires a fresh, open market. Fully exhausted collateral can leave bad debt: its publicly recognized loss reduces lender share value, and the borrower still owes the shortfall. Lenders carry collateral, feed, market closure, issuer transfer/freeze and USDT risks. Existing `TabBacking` custody does not automatically become collateral in this module.

`TabMarketHours` provides a default-closed adapter. Its immutable operator must attest issuer trading/transfer availability with a source hash and an expiry no more than 15 minutes away. An expired attestation closes the market. This is an operator attestation rather than an autonomous exchange calendar; a production operator and trustworthy issuer-status source must be configured before stock borrowing can be enabled.

`TabBuyback` accepts explicit, irreversible USDT contributions. Its immutable router and route must end at the existing official `TabProtocol.tabToken()`, so an unconfigured official token prevents deployment. Only the protocol authority executes purchases. Each execution checks a live router quote, the configured maximum slippage, an exact spend, actual received tokens and a deadline no more than ten minutes away. Router allowance is reset to zero after each purchase. Acquired tokens go to the fixed dead address; this does not call a token burn function or claim a change to total supply. A spot router quote is a slippage check, not a manipulation-resistant price oracle. Direct donations are excluded from spendable funding.

The old protocol's fee reserve cannot be extracted by these modules. Its buyback counters remain separate from `TabBuyback` purchase counters. A separately funded buyback is implemented; automatic use of the legacy reserve is not. No administrator can seize pool assets or borrower collateral, rescue custody tokens, redirect the buyback route or withdraw buyback contributions. Pausing prevents fresh borrowing/swaps while keeping lender redemption and repayment available.

### Finance deployment plan

1. Review the production protocol/authority, exact Binance-Peg USDT and each new runtime bytecode. Deploy `TabLendingPool(protocolAddress)` only after accepting its unsecured underwriting and redemption rules.
2. Identify actual issuer-approved collateral tokens and deployed collateral/USD and USDT/USD feeds on chain 56. Verify token decimals, transfer behavior, feed decimals/heartbeats and the usable market-status source. Deploy `TabMarketHours(operator)` and `TabStockLending(protocolAddress, usdtUsdFeed, usdtMaxAge)`. Start with an empty whitelist and closed markets. A whitelist configuration is a separate reviewed authority transaction.
3. Verify and configure the existing official TAB token on the protocol if required. Review router liquidity and the fixed USDT-to-TAB path before deploying `TabBuyback(protocolAddress, routerAddress, path, maxSlippageBps)`.
4. Record addresses, code hashes, immutable getters, oracle/router hashes and asset risk policy in `backend/config/finance-bnb.json`. Keep module status unavailable until these checks agree with live RPC state.
5. Review a concrete transaction preview and gas quote for every deployment/configuration/funding action. First liquidity, collateral, advances and buyback funding are separate user-authorized transactions. This plan does not sign or submit them.

The public read-only planner below verifies live chain 56, the immutable protocol/USDT hashes and authority, checks its pending nonce twice, simulates the reviewed pool constructor to obtain its actual runtime hash, and estimates deployment gas. It writes a concrete unsigned deployment transaction to `contracts/deployments/bnb-finance-plan.json`. A 20% gas buffer and hard 0.00022 BNB fee ceiling apply. It uses public RPC and never imports a signer or broadcasts a transaction. Stock lending and buyback stay explicitly blocked until their actual oracle/operator/TAB/router choices are reviewed. Refresh the plan immediately before any separate signing approval:

```sh
node contracts/bnb/scripts/plan-finance-deployment.mjs
```

`scripts/execute-lending-deployment.mjs` is a disabled, narrowly pinned executor for that separately reviewed lending-only transaction. It requires `TAB_AUTHORIZE_LENDING_DEPLOYMENT` to equal the reviewed creation-data hash and accepts the project key only through Ryan Vault's `TAB_BNB_DEPLOY_KEY` environment injection. It pins nonce, predicted address, source/compiler/artifact, constructor/runtime, protocol, USDT and authority; reruns fresh chain/nonce/balance/gas/simulation checks; and refuses fees above 0.00022 BNB. A kernel lock prevents concurrent signing. Before broadcasting, it durably writes the signed transaction to the Git-ignored private `contracts/bnb/broadcast/finance-lending/journal.json` with restricted permissions. Recovery reuses those exact signed bytes, without a new nonce, price bump or signature. After twelve canonical confirmations and exact transaction/runtime/getter checks, it writes a public `contracts/deployments/bnb-finance-deployed.json` receipt manifest. It never funds liquidity, approves loans, deploys stock/buyback modules or activates backend configuration. Preparing this helper does not authorize execution.

Local deployment gas measurements with Solidity 0.8.28, optimizer 200 and Paris opcodes are approximately 3,580,215 gas for the working-capital pool, 3,466,908 gas for stock lending, 280,622 gas for market hours and 1,482,887 gas for buyback, or about 8.81 million gas together. Mock feeds/tokens were used; production feed reads, bytecode, constructor inputs and configuration transactions may differ. At an illustrative 1 gwei, that sum is about 0.00881 BNB before configurations. Obtain current chain-56 estimates and gas price before approval; this is not a live fee quote. Runtime bytecode sizes are 15,869, 15,249, 1,045 and 6,050 bytes respectively, below the 24,576-byte deployment limit.

The finance suites are `TabWorkingCapitalTest`, `TabStockLendingTest`, `TabBuybackTest` and `TabMarketHoursTest`. They cover actual funded principal, available-cash redemption, job/signer restrictions, receipts/replay, default recognition/recovery, USDT depeg conversion, oracle failures, market expiry, liquidation bonuses/shortfalls, unsafe token transfers, reentrancy, fresh buyback funding, actual output/slippage/deadline and funding conservation. Finance fuzz tests use 1,024 cases. Run them against the current checkout with:

```sh
forge test --match-contract 'TabWorkingCapitalTest|TabStockLendingTest|TabBuybackTest|TabMarketHoursTest' -vv
BNB_FORK_RPC_URL=https://bsc-dataseed.bnbchain.org forge test --match-contract TabFinanceForkTest -vv
```

The opt-in BNB fork suites exercise canonical USDT and the protocol selected by the deployment fixtures. They cover working-capital interoperability, canonical-USDT pool repayment/redemption, and explicit buyback funding. Verify that fixture addresses match the deployment being assessed before interpreting a result. The stock token, oracle, router and official-token configuration are simulated locally; these tests do not validate a production stock issuer, price feed or TAB trading route. All wallet impersonation, USDT balance overrides and module deployments remain inside the local Foundry fork. Historical test snapshots in the excluded future directory are not part of the active suite.

The new module ABIs are checked in under `abi/TabLendingPool.json`, `abi/TabStockLending.json`, `abi/TabMarketHours.json` and `abi/TabBuyback.json`. Regenerate these from their own `out/<Name>.sol/<Name>.json` artifacts without replacing ABIs for the immutable deployed modules.
