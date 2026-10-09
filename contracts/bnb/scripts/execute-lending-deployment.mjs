#!/usr/bin/env node
// Disabled until an explicit hash-scoped authorization is injected. Deploys only the reviewed pool.
// Run through Ryan Vault after approval. Never pass a key in arguments, a file or a public environment file.
import { readFile, writeFile, mkdir, open, rename, lstat, chmod } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import { createRequire } from "node:module";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

const AUTHORIZATION_HASH = "0x1b5d34bc4cc0c5f1d44b287db5f61b20cea06f86d96d14eb9478643626376298";
// No key lookup, network request, signing or broadcasting occurs before this guard.
if (process.env.TAB_AUTHORIZE_LENDING_DEPLOYMENT !== AUTHORIZATION_HASH) {
  throw Error("Deployment is disabled. Explicit authorization must match the reviewed creation-data hash.");
}
const require = createRequire(new URL("../../../frontend/package.json", import.meta.url));
const {
  createPublicClient, http, getAddress, encodeDeployData, parseAbi, keccak256,
  getContractAddress, parseTransaction, recoverTransactionAddress, formatEther,
} = require("viem");
const root = new URL("../../../", import.meta.url);
const privateDir = new URL("contracts/bnb/broadcast/finance-lending/", root);
const journalPath = new URL("journal.json", privateDir);
const publicManifestPath = new URL("contracts/deployments/bnb-finance-deployed.json", root);
const pinned = {
  authority: getAddress("0x00e128e7779ea927a087b40ade268dedc1b34a90"),
  protocol: getAddress("0x567e7187d477b1a68c3ac3d292ad44a0d5e770c7"),
  protocolHash: "0xb2d365466cb7db891382003d25c65908733c3eb6f3434da02934017d4587ecfd",
  usdt: getAddress("0x55d398326f99059fF775485246999027B3197955"),
  usdtHash: "0x97a48aa4c129657440dafdacd4c836389734d28cc4a0ca7403e68da660a74a59",
  artifactSHA: "e68b2446d8ca63571968a4cea96612fc75ec9d7c8e80defdabdd206ce143035a",
  sourceSHA: "04c9558ea2b3ad0401febdd8f90eac85febad1750519289f700b7b6070d49a77",
  runtimeHash: "0xedaa82bde4e510fbb444e7d72d939057b38ce4c3dca9d13089c98860377d970a",
  runtimeBytes: 15_869,
  nonce: 10,
  address: getAddress("0xbe2c140c0b40d25ef5531d93c0696318319e6375"),
};
const feeCeiling = 220_000_000_000_000n; // Hard limit: 0.00022 BNB. Value and USDT spending are zero.
const confirmations = 12;
const sha = bytes => createHash("sha256").update(bytes).digest("hex");
const canonical = value => Array.isArray(value) ? value.map(canonical) : value && typeof value === "object"
  ? Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])])) : value;
const json = value => JSON.stringify(value, (_, v) => typeof v === "bigint" ? v.toString() : v, 2);

async function privateStorage() {
  // Git must already ignore the journal, rather than accepting an after-the-fact ignore rule.
  execFileSync("git", ["check-ignore", "--quiet", fileURLToPath(journalPath)], { cwd: fileURLToPath(root), stdio: "ignore" });
  await mkdir(privateDir, { mode: 0o700, recursive: true });
  const stat = await lstat(privateDir);
  if (!stat.isDirectory() || stat.isSymbolicLink()) throw Error("Private journal directory is invalid.");
  await chmod(privateDir, 0o700);
}

// A kernel lock prevents two copies from signing different transactions during concurrent execution.
// The executing process retains the open file description that flock locks through inherited fd 3.
// There is no argument that bypasses this acquisition, and child exit does not close the parent's descriptor.
if (process.argv.length !== 2) throw Error("No deployment arguments are accepted.");
await privateStorage();
const lockPath = new URL("execute.lock", privateDir);
const lockFile = await open(lockPath, fsConstants.O_CREAT | fsConstants.O_RDWR | fsConstants.O_NOFOLLOW, 0o600);
await lockFile.chmod(0o600);
try { execFileSync("flock", ["-n", "3"], { stdio: ["ignore", "ignore", "ignore", lockFile.fd] }); }
catch { await lockFile.close(); throw Error("Another lending deployment executor holds the private journal lock."); }

async function journalWrite(value) {
  const temp = new URL(`journal.${process.pid}.tmp`, privateDir);
  const file = await open(temp, "wx", 0o600);
  try { await file.writeFile(`${json(value)}\n`); await file.sync(); } finally { await file.close(); }
  await rename(temp, journalPath);
  const directory = await open(privateDir, "r");
  try { await directory.sync(); } finally { await directory.close(); }
}
async function journalRead() {
  try {
    const stat = await lstat(journalPath);
    if (!stat.isFile() || stat.isSymbolicLink() || (stat.mode & 0o077) !== 0) throw Error("Private journal permissions are unsafe.");
    return JSON.parse(await readFile(journalPath, "utf8"));
  } catch (error) { if (error.code === "ENOENT") return null; throw error; }
}

const plan = JSON.parse(await readFile(new URL("contracts/deployments/bnb-finance-plan.json", root), "utf8"));
const artifactRaw = await readFile(new URL("contracts/bnb/out/TabLendingPool.sol/TabLendingPool.json", root));
const artifact = JSON.parse(artifactRaw.toString("utf8"));
if (sha(artifactRaw) !== pinned.artifactSHA || sha(JSON.stringify(canonical(artifact.metadata.sources))) !== pinned.sourceSHA
    || artifact.metadata.compiler.version !== "0.8.28+commit.7893614a") throw Error("Reviewed source/compiler/artifact changed. Prepare and review a new deployment.");
const data = encodeDeployData({ abi: artifact.abi, bytecode: artifact.bytecode.object, args: [pinned.protocol] });
if (keccak256(data) !== AUTHORIZATION_HASH || plan.chain_id !== 56 || plan.lending.creation_data_hash !== AUTHORIZATION_HASH
    || getAddress(plan.authority) !== pinned.authority || getAddress(plan.protocol.address) !== pinned.protocol
    || plan.protocol.code_hash !== pinned.protocolHash || getAddress(plan.settlement.address) !== pinned.usdt
    || plan.settlement.code_hash !== pinned.usdtHash || plan.settlement.decimals !== 18
    || plan.lending.simulated_runtime_code_hash !== pinned.runtimeHash || Number(plan.nonce.pending) !== pinned.nonce
    || getAddress(plan.lending.proposed_address) !== pinned.address || plan.lending.transaction.data !== data
    || plan.lending.transaction.value !== "0x0") throw Error("Public plan differs from the exact authorized deployment.");
if (!["https://bsc-dataseed.bnbchain.org", "https://bsc-rpc.publicnode.com"].includes(plan.rpc_endpoint)) throw Error("Only a public pinned RPC endpoint is permitted.");
const client = createPublicClient({ transport: http(plan.rpc_endpoint, { timeout: 20_000, retryCount: 1 }) });
const protocolABI = parseAbi(["function usdt() view returns(address)", "function authority() view returns(address)"]);
const tokenABI = parseAbi(["function decimals() view returns(uint8)"]);
const [chainId, protocolCode, usdtCode, authority, usdt, decimals, runtime] = await Promise.all([
  client.getChainId(), client.getBytecode({ address: pinned.protocol }), client.getBytecode({ address: pinned.usdt }),
  client.readContract({ address: pinned.protocol, abi: protocolABI, functionName: "authority" }),
  client.readContract({ address: pinned.protocol, abi: protocolABI, functionName: "usdt" }),
  client.readContract({ address: pinned.usdt, abi: tokenABI, functionName: "decimals" }),
  client.request({ method: "eth_call", params: [{ from: pinned.authority, data, value: "0x0" }, "latest"] }),
]);
if (chainId !== 56 || !protocolCode || keccak256(protocolCode) !== pinned.protocolHash || !usdtCode || keccak256(usdtCode) !== pinned.usdtHash
    || getAddress(authority) !== pinned.authority || getAddress(usdt) !== pinned.usdt || decimals !== 18
    || keccak256(runtime) !== pinned.runtimeHash || (runtime.length - 2) / 2 !== pinned.runtimeBytes) throw Error("Live chain, authority, token, bytecode or constructor simulation changed.");

let journal = await journalRead();
if (!journal) {
  const planAge = Date.now() - Date.parse(plan.prepared_at);
  if (!Number.isFinite(planAge) || planAge < 0 || planAge > 10 * 60_000 || plan.status !== "prepared_deploy_only_requires_authorization") throw Error("Prepare a fresh unsigned plan before requesting deployment authorization.");
  const [nonce, latestNonce, gasEstimate, gasPrice, balance] = await Promise.all([
    client.getTransactionCount({ address: pinned.authority, blockTag: "pending" }),
    client.getTransactionCount({ address: pinned.authority, blockTag: "latest" }),
    client.estimateGas({ account: pinned.authority, data, value: 0n }),
    client.getGasPrice(), client.getBalance({ address: pinned.authority, blockTag: "pending" }),
  ]);
  const gas = (gasEstimate * 120n + 99n) / 100n;
  if (nonce !== Number(plan.nonce.pending) || latestNonce !== nonce || gasPrice <= 0n || gas * gasPrice > feeCeiling
      || balance < gas * gasPrice) throw Error("Nonce, gas price, fee ceiling or available BNB changed. No transaction was signed.");
  if (await client.getTransactionCount({ address: pinned.authority, blockTag: "pending" }) !== nonce) throw Error("Authority nonce changed before signing.");
  const key = process.env.TAB_BNB_DEPLOY_KEY;
  if (typeof key !== "string" || !/^0x[0-9a-fA-F]{64}$/.test(key)) throw Error("Inject the project deployment key through Ryan Vault only.");
  delete process.env.TAB_BNB_DEPLOY_KEY;
  const { privateKeyToAccount } = require("viem/accounts");
  let account;
  try { account = privateKeyToAccount(key); } catch { throw Error("Ryan Vault's project deployment key is invalid."); }
  if (getAddress(account.address) !== pinned.authority) throw Error("The project deployment key belongs to another authority.");
  let raw;
  try { raw = await account.signTransaction({ chainId: 56, type: "legacy", nonce, data, value: 0n, gas, gasPrice }); }
  catch { throw Error("Bounded deployment signing failed. No transaction was broadcast."); }
  journal = { schema_version: 1, status: "signed_not_broadcast", signed_at: new Date().toISOString(), chain_id: 56,
    authority: pinned.authority, nonce, gas: gas.toString(), gas_price: gasPrice.toString(), hard_fee_ceiling_wei: feeCeiling.toString(),
    creation_data_hash: AUTHORIZATION_HASH, artifact_sha256: pinned.artifactSHA, source_manifest_sha256: pinned.sourceSHA,
    runtime_code_hash: pinned.runtimeHash, predicted_address: getContractAddress({ from: pinned.authority, nonce: BigInt(nonce) }),
    tx_hash: keccak256(raw), signed_raw_transaction: raw };
  // Durable private storage precedes any network submission. Recovery uses these same signed bytes only.
  await journalWrite(journal);
}
const raw = journal.signed_raw_transaction;
let parsed, recovered;
try { parsed = parseTransaction(raw); recovered = await recoverTransactionAddress({ serializedTransaction: raw }); }
catch { throw Error("The private signed journal is invalid. No transaction was broadcast."); }
if (journal.schema_version !== 1 || journal.creation_data_hash !== AUTHORIZATION_HASH || journal.artifact_sha256 !== pinned.artifactSHA
    || journal.source_manifest_sha256 !== pinned.sourceSHA || keccak256(raw) !== journal.tx_hash
    || journal.runtime_code_hash !== pinned.runtimeHash || getAddress(recovered) !== pinned.authority
    || parsed.chainId !== 56 || parsed.type !== "legacy" || parsed.to || (parsed.value ?? 0n) !== 0n
    || parsed.data !== data || parsed.nonce !== journal.nonce || parsed.nonce !== pinned.nonce || parsed.gas !== BigInt(journal.gas)
    || parsed.gasPrice !== BigInt(journal.gas_price) || parsed.gas * parsed.gasPrice > feeCeiling
    || journal.predicted_address !== getContractAddress({ from: pinned.authority, nonce: BigInt(journal.nonce) })
    || getAddress(journal.predicted_address) !== pinned.address) throw Error("Private journal does not contain the one authorized bounded deployment.");

let receipt = await client.getTransactionReceipt({ hash: journal.tx_hash }).catch(() => null);
if (!receipt) {
  const latestNonce = await client.getTransactionCount({ address: pinned.authority, blockTag: "latest" });
  if (latestNonce > journal.nonce) throw Error("Nonce was consumed without this deployment receipt. Keep the journal for investigation; no new signature is permitted.");
  // Rebroadcasting identical signed bytes cannot create another deployment or increase the fee.
  journal.status = "broadcast_uncertain";
  journal.last_attempt_at = new Date().toISOString();
  await journalWrite(journal);
  try {
    const broadcastHash = await client.sendRawTransaction({ serializedTransaction: raw });
    if (broadcastHash !== journal.tx_hash) throw Error("RPC transaction hash differs from the journal.");
    journal.status = "submitted";
    await journalWrite(journal);
  } catch {
    // An RPC timeout/already-known response never authorizes a new transaction or fee bump.
    journal.status = "broadcast_uncertain";
    await journalWrite(journal);
  }
}
try {
  receipt = await client.waitForTransactionReceipt({ hash: journal.tx_hash, confirmations, timeout: 180_000, pollingInterval: 2_000 });
} catch {
  journal.status = "pending_or_uncertain";
  await journalWrite(journal);
  throw Error("Deployment confirmation is pending. Re-run only this helper with the same authorization; it will reuse the journal transaction.");
}
const tx = await client.getTransaction({ hash: journal.tx_hash });
const canonicalBlock = await client.getBlock({ blockNumber: receipt.blockNumber });
const head = await client.getBlockNumber();
if (receipt.transactionHash !== journal.tx_hash || receipt.status !== "success" || receipt.blockHash !== canonicalBlock.hash
    || head - receipt.blockNumber + 1n < BigInt(confirmations) || getAddress(receipt.contractAddress) !== getAddress(journal.predicted_address)
    || getAddress(tx.from) !== pinned.authority || tx.to || tx.input !== data || tx.value !== 0n || tx.chainId !== 56
    || tx.nonce !== journal.nonce || tx.gas !== parsed.gas || tx.gasPrice !== parsed.gasPrice
    || receipt.gasUsed * receipt.effectiveGasPrice > feeCeiling) throw Error("Final receipt differs from the exact authorized creation transaction.");
const deployed = getAddress(receipt.contractAddress);
const [code, protocolGetter, usdtGetter, ownerGetter, assetGetter, pool] = await Promise.all([
  client.getBytecode({ address: deployed }),
  client.readContract({ address: deployed, abi: artifact.abi, functionName: "protocol" }),
  client.readContract({ address: deployed, abi: artifact.abi, functionName: "usdt" }),
  client.readContract({ address: deployed, abi: artifact.abi, functionName: "underwriter" }),
  client.readContract({ address: deployed, abi: artifact.abi, functionName: "asset" }),
  client.readContract({ address: deployed, abi: artifact.abi, functionName: "getPool" }),
]);
if (!code || keccak256(code) !== pinned.runtimeHash || getAddress(protocolGetter) !== pinned.protocol
    || getAddress(usdtGetter) !== pinned.usdt || getAddress(assetGetter) !== pinned.usdt || getAddress(ownerGetter) !== pinned.authority
    || getAddress(pool.asset) !== pinned.usdt || getAddress(pool.underwriter) !== pinned.authority) throw Error("Deployed pool runtime or immutable getters differ from the reviewed module.");
// Recovery of this lending receipt must preserve separately deployed modules in the public manifest.
let priorManifest = null;
try { priorManifest = JSON.parse(await readFile(publicManifestPath, "utf8")); }
catch (error) { if (error.code !== "ENOENT") throw error; }
if (priorManifest && (priorManifest.schema_version !== 1 || priorManifest.chain_id !== 56
    || getAddress(priorManifest.protocol) !== pinned.protocol || getAddress(priorManifest.authority) !== pinned.authority
    || getAddress(priorManifest.usdt_address) !== pinned.usdt || priorManifest.usdt_decimals !== 18
    || (priorManifest.modules?.lending?.address && getAddress(priorManifest.modules.lending.address) !== deployed))) {
  throw Error("Existing deployment manifest belongs to different reviewed bindings. Preserve it and merge this lending receipt after review.");
}
const manifest = { schema_version: 1, status: "deployed_verified", verified_at: new Date().toISOString(), chain_id: 56,
  protocol: pinned.protocol, authority: pinned.authority, usdt_address: pinned.usdt, usdt_decimals: 18,
  modules: { lending: { address: deployed, code_hash: pinned.runtimeHash, transaction_hash: journal.tx_hash,
    block_number: receipt.blockNumber.toString(), block_hash: receipt.blockHash, confirmations, constructor_arguments: [pinned.protocol],
    source_manifest_sha256: pinned.sourceSHA, artifact_sha256: pinned.artifactSHA, creation_data_hash: AUTHORIZATION_HASH,
    gas_used: receipt.gasUsed.toString(), gas_price_wei: receipt.effectiveGasPrice.toString(), gas_paid_bnb: formatEther(receipt.gasUsed * receipt.effectiveGasPrice),
    pool_at_verification: pool }, stock_loans: priorManifest?.modules?.stock_loans ?? { address: null, code_hash: null },
    buyback: priorManifest?.modules?.buyback ?? { address: null, code_hash: null } },
  scope: { liquidity_funded_by_executor_usdt: "0", loans_approved: 0, collateral_deposited: "0", buyback_funding: "0", backend_configuration_changed: false },
};
const manifestTemp = new URL(`contracts/deployments/bnb-finance-deployed.${process.pid}.tmp`, root);
await writeFile(manifestTemp, `${json(manifest)}\n`, { flag: "wx" });
await rename(manifestTemp, publicManifestPath);
journal.status = "verified";
journal.verified_at = manifest.verified_at;
await journalWrite(journal);
console.log(json({ status: "deployed_verified", address: deployed, transaction_hash: journal.tx_hash,
  gas_paid_bnb: manifest.modules.lending.gas_paid_bnb, fee_ceiling_bnb: "0.00022", manifest: fileURLToPath(publicManifestPath), liquidity_funded: "0" }));
await lockFile.close();
