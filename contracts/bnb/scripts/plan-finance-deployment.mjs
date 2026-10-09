#!/usr/bin/env node
// Public, read-only deployment planner. No signer, private key or send-transaction method.
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { createRequire } from "node:module";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";

const require = createRequire(new URL("../../../frontend/package.json", import.meta.url));
const {
  createPublicClient, http, getAddress, encodeDeployData, parseAbi, keccak256,
  getContractAddress, formatEther, toHex,
} = require("viem");

const PUBLIC_RPC = new Set([
  "https://bsc-dataseed.bnbchain.org",
  "https://bsc-rpc.publicnode.com",
]);
const rpc = process.argv[2] || "https://bsc-dataseed.bnbchain.org";
if (!PUBLIC_RPC.has(rpc)) throw Error("Choose one of the planner's public RPC endpoints. Authenticated URLs are not accepted.");
if (process.argv.length > 3) throw Error("Usage: node contracts/bnb/scripts/plan-finance-deployment.mjs [public_rpc_url]");

const root = new URL("../../../", import.meta.url);
const manifestPath = new URL("contracts/deployments/bnb-56.json", root);
const artifactPath = new URL("contracts/bnb/out/TabLendingPool.sol/TabLendingPool.json", root);
const outputPath = new URL("contracts/deployments/bnb-finance-plan.json", root);
const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
const artifactRaw = await readFile(artifactPath);
const artifact = JSON.parse(artifactRaw.toString("utf8"));
const protocol = getAddress(manifest.contracts.protocol.address);
const expectedAuthority = getAddress(manifest.authority);
const canonicalUSDT = getAddress("0x55d398326f99059fF775485246999027B3197955");
if (manifest.status !== "deployed" || manifest.chain_id !== 56 || getAddress(manifest.usdt_address) !== canonicalUSDT) {
  throw Error("The existing deployment manifest must identify verified BNB mainnet and canonical USDT.");
}
if (artifact.metadata.compiler.version !== "0.8.28+commit.7893614a") throw Error("Compile the reviewed Solidity 0.8.28 artifact first.");
const bytecode = artifact.bytecode.object;
if (typeof bytecode !== "string" || !/^0x[0-9a-f]+$/i.test(bytecode) || bytecode.length < 100) {
  throw Error("The lending creation bytecode is missing or has unresolved library links.");
}
const client = createPublicClient({ transport: http(rpc, { timeout: 20_000, retryCount: 1 }) });
const protocolABI = parseAbi(["function usdt() view returns(address)", "function authority() view returns(address)"]);
const usdtABI = parseAbi(["function decimals() view returns(uint8)"]);
const [chainId, block, protocolCode, usdtCode, authorityRaw, usdtRaw, usdtDecimals] = await Promise.all([
  client.getChainId(), client.getBlock(), client.getBytecode({ address: protocol }),
  client.getBytecode({ address: canonicalUSDT }),
  client.readContract({ address: protocol, abi: protocolABI, functionName: "authority" }),
  client.readContract({ address: protocol, abi: protocolABI, functionName: "usdt" }),
  client.readContract({ address: canonicalUSDT, abi: usdtABI, functionName: "decimals" }),
]);
const authority = getAddress(authorityRaw);
if (chainId !== 56 || authority !== expectedAuthority || getAddress(usdtRaw) !== canonicalUSDT || usdtDecimals !== 18) {
  throw Error("Live chain, protocol authority or 18-decimal USDT differs from the deployment manifest.");
}
if (!protocolCode || keccak256(protocolCode) !== manifest.contracts.protocol.code_hash.toLowerCase()) {
  throw Error("Live protocol runtime code differs from the approved immutable deployment.");
}
if (!usdtCode || keccak256(usdtCode) !== manifest.usdt_code_hash.toLowerCase()) {
  throw Error("Canonical USDT runtime code differs from the deployment manifest.");
}
const data = encodeDeployData({ abi: artifact.abi, bytecode, args: [protocol] });
const [noncePending, nonceLatest, gasEstimate, gasPrice, runtime, authorityBalance] = await Promise.all([
  client.getTransactionCount({ address: authority, blockTag: "pending" }),
  client.getTransactionCount({ address: authority, blockTag: "latest" }),
  client.estimateGas({ account: authority, data, value: 0n }),
  client.getGasPrice(),
  client.request({ method: "eth_call", params: [{ from: authority, data, value: "0x0" }, "latest"] }),
  client.getBalance({ address: authority, blockTag: "pending" }),
]);
if (typeof runtime !== "string" || !/^0x[0-9a-f]+$/i.test(runtime)) throw Error("Constructor simulation did not return deployment runtime.");
const runtimeBytes = (runtime.length - 2) / 2;
if (!runtimeBytes || runtimeBytes > 24_576) throw Error("The simulated lending runtime exceeds the deployment size limit.");
const gasLimit = (gasEstimate * 120n + 99n) / 100n;
const feeCeiling = 220_000_000_000_000n; // Narrow reviewed ceiling: 0.00022 BNB.
const maximumAtPrice = gasLimit * gasPrice;
const feeWithinCeiling = maximumAtPrice <= feeCeiling;
const enoughGasBalance = authorityBalance >= maximumAtPrice;
const maximumGasPrice = feeCeiling / gasLimit;
const latestNonceCheck = await client.getTransactionCount({ address: authority, blockTag: "pending" });
if (latestNonceCheck !== noncePending) throw Error("The authority pending nonce changed while planning. Refresh the plan.");
const proposedAddress = getContractAddress({ from: authority, nonce: BigInt(noncePending) });
const output = {
  schema_version: 1,
  status: noncePending !== nonceLatest ? "blocked_pending_nonce" : !feeWithinCeiling ? "blocked_gas_fee_ceiling" : !enoughGasBalance ? "blocked_authority_gas_balance" : "prepared_deploy_only_requires_authorization",
  prepared_at: new Date().toISOString(),
  chain_id: 56,
  network: "BNB Smart Chain mainnet",
  rpc_endpoint: rpc,
  block_number: block.number.toString(),
  block_hash: block.hash,
  block_timestamp: block.timestamp.toString(),
  protocol: { address: protocol, code_hash: keccak256(protocolCode) },
  authority,
  authority_balance_bnb: formatEther(authorityBalance),
  settlement: { address: canonicalUSDT, decimals: usdtDecimals, code_hash: keccak256(usdtCode) },
  nonce: { latest: nonceLatest.toString(), pending: noncePending.toString(), checked_again_before_write: true },
  lending: {
    contract: "TabLendingPool",
    proposed_address: proposedAddress,
    actual_deployed_address: null,
    constructor_arguments: [protocol],
    artifact_path: "contracts/bnb/out/TabLendingPool.sol/TabLendingPool.json",
    artifact_sha256: createHash("sha256").update(artifactRaw).digest("hex"),
    compiler: artifact.metadata.compiler.version,
    creation_data_hash: keccak256(data),
    simulated_runtime_code_hash: keccak256(runtime),
    simulated_runtime_bytes: runtimeBytes,
    transaction: { from: authority, to: null, chainId: "0x38", nonce: toHex(noncePending), data, value: "0x0", gas: toHex(gasLimit), gasPrice: toHex(gasPrice) },
    gas: {
      estimate: gasEstimate.toString(), limit_with_20_percent_buffer: gasLimit.toString(),
      current_price_wei: gasPrice.toString(), maximum_price_within_ceiling_wei: maximumGasPrice.toString(),
      estimated_fee_bnb: formatEther(gasEstimate * gasPrice), maximum_fee_at_current_price_bnb: formatEther(maximumAtPrice),
      hard_fee_ceiling_wei: feeCeiling.toString(), hard_fee_ceiling_bnb: "0.00022", within_ceiling: feeWithinCeiling,
      authority_has_enough_bnb_at_current_price: enoughGasBalance,
    },
  },
  blocked_modules: {
    stock_loans: { proposed_address: null, reason: "Requires exact reviewed collateral and USDT/USD feeds, issuer collateral token, historical risk policy and market-status operator/source. No choices are invented by this planner." },
    market_hours: { proposed_address: null, reason: "Requires an explicit reviewed operator and issuer-status source. Starts closed." },
    buyback: { proposed_address: null, reason: "Requires independently verified official TAB configuration, fixed router and trading route. Existing protocol fee reserve is inaccessible." },
  },
  effects: { broadcast: false, signing: false, usdt_spending: "0", liquidity_funding: "0", loan_approval: false, backend_manifest_switch: false },
  next_step: "Review and authorize only this deployment transaction. Recheck chain, bytecode, pending nonce, fee ceiling and authority immediately before signing. Funding or borrower approvals require separate transactions.",
};
await mkdir(new URL("contracts/deployments/", root), { recursive: true });
await writeFile(outputPath, `${JSON.stringify(output, null, 2)}\n`);
console.log(JSON.stringify({ status: output.status, path: fileURLToPath(outputPath), authority, pending_nonce: noncePending,
  proposed_lending_address: proposedAddress, gas_limit: gasLimit.toString(), maximum_fee_bnb: formatEther(maximumAtPrice),
  hard_fee_ceiling_bnb: "0.00022", runtime_bytes: runtimeBytes, simulated_runtime_code_hash: keccak256(runtime), broadcast: false }, null, 2));
