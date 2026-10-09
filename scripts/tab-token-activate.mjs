#!/usr/bin/env node
// One irreversible configureTab call, for Ryan's supplied existing token only.
// plan and simulate are read-only onchain; execute consumes TAB_DEPLOYER_KEY
// through Ryan Vault. No token transfer, approval, swap or deployment is allowed.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createPublicClient, http, parseAbi, erc20Abi, encodeFunctionData, decodeEventLog, keccak256, parseTransaction, recoverTransactionAddress, formatEther } from '../frontend/node_modules/viem/_esm/index.js';
import { privateKeyToAccount } from '../frontend/node_modules/viem/_esm/accounts/index.js';
import { bsc } from '../frontend/node_modules/viem/_esm/chains/index.js';
import { SOURCE, PROTOCOL, USDT, PINS, digest, validateManifest } from './finance-pilot.mjs';

export const TOKEN = '0xf07449517ae4b48808098c573a5347e67c714444';
export const TOKEN_HASH = '0x01f0430e2bbc285e7a55cc7409977eccb1b7b944068956cffcb7dac5c75b3d1f';
export const IMPLEMENTATION = '0x46862924e2a229170ebd065e24a0da72af58a986';
export const IMPLEMENTATION_HASH = '0xa5529c201b5fdf90cdcddd546521c4e75374e47fdb1aa356859307a4532da97f';
export const GAS_LIMIT = 100_000n;
export const GAS_CAP = 200_000_000_000_000n; // 0.0002 BNB, within remaining pilot cap.
export const ABI = parseAbi(['function configureTab(address token)', 'function tabToken() view returns(address)', 'function authority() view returns(address)', 'function usdt() view returns(address)', 'event TabConfigured(address token)']);
export const TOKEN_FIELDS = Object.freeze({ official_tab_address: TOKEN, official_tab_code_hash: TOKEN_HASH, official_tab_implementation_address: IMPLEMENTATION, official_tab_implementation_code_hash: IMPLEMENTATION_HASH, official_tab_name: 'Tabx402', official_tab_symbol: 'TAB', official_tab_decimals: 18 });
const ZERO = '0x0000000000000000000000000000000000000000';
const ID = 'tab-bnb56-official-token-f0744951-v1';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const directory = '/home/ubuntu/.local/state/tabagents/tab-token-activation-56';
const files = { manifest: path.join(root, 'contracts/deployments/bnb-56.json'), plan: path.join(root, 'contracts/deployments/bnb-tab-activation-56.json'), proof: path.join(root, 'contracts/deployments/bnb-tab-activation-56-receipt.json'), journal: path.join(directory, 'signed-transaction.json') };
const eq = (a, b) => typeof a === 'string' && typeof b === 'string' && a.toLowerCase() === b.toLowerCase();
function guard(ok, message) { if (!ok) throw new Error(`TAB activation: ${message}`); }
function read(file) { return JSON.parse(fs.readFileSync(file, 'utf8')); }
function write(file, data, mode = 0o644) {
  const temp = `${file}.${process.pid}.tmp`, fd = fs.openSync(temp, 'wx', mode);
  try { fs.writeFileSync(fd, `${JSON.stringify(data, null, 2)}\n`); fs.fsyncSync(fd); } finally { fs.closeSync(fd); }
  fs.renameSync(temp, file);
  const dir = fs.openSync(path.dirname(file), 'r'); try { fs.fsyncSync(dir); } finally { fs.closeSync(dir); }
}
export function manifestCore(manifest) {
  validateManifest(manifest);
  for (const [key, expected] of Object.entries(TOKEN_FIELDS)) guard(manifest[key] == null || manifest[key] === expected, `staged ${key} differs from the supplied token`);
  // Token fields may be pre-staged for the release before this one-time call.
  return { chain_id: manifest.chain_id, authority: manifest.authority, source_hash: manifest.source_hash, usdt_address: manifest.usdt_address, usdt_code_hash: manifest.usdt_code_hash, contracts: manifest.contracts };
}
export function transaction(nonce, gasPrice) {
  return { chainId: 56, type: 'legacy', to: PROTOCOL, value: 0n, data: encodeFunctionData({ abi: ABI, functionName: 'configureTab', args: [TOKEN] }), nonce, gas: GAS_LIMIT, gasPrice: BigInt(gasPrice) };
}
export function buildPlan(manifest, nonce, gasPrice, observedBlock, now = new Date()) {
  const plan = { id: ID, chain_id: 56, from: SOURCE, to: PROTOCOL, token: TOKEN_FIELDS, core_hash: digest(manifestCore(manifest)), nonce, gas: GAS_LIMIT.toString(), gas_price_wei: String(gasPrice), value: '0', data: transaction(nonce, gasPrice).data, maximum_gas_wei: GAS_CAP.toString(), observed_block: String(observedBlock), created_at: now.toISOString(), irreversible: true, scope: 'Configure the existing official TAB token once. No funds, approvals, swaps or deployments.' };
  validatePlan(plan, manifest); return plan;
}
export function validatePlan(plan, manifest) {
  guard(plan.id === ID && plan.chain_id === 56 && eq(plan.from, SOURCE) && eq(plan.to, PROTOCOL), 'wrong activation target');
  guard(plan.core_hash === digest(manifestCore(manifest)) && JSON.stringify(plan.token) === JSON.stringify(TOKEN_FIELDS), 'deployment or token pins changed');
  guard(Number.isSafeInteger(plan.nonce) && plan.nonce >= 0, 'invalid nonce');
  guard(typeof plan.gas_price_wei === 'string' && /^[1-9]\d*$/.test(plan.gas_price_wei) && BigInt(plan.gas_price_wei) <= 1_000_000_000n, 'gas price exceeds 1 gwei');
  guard(plan.gas === GAS_LIMIT.toString() && GAS_LIMIT * BigInt(plan.gas_price_wei) <= GAS_CAP && plan.maximum_gas_wei === GAS_CAP.toString(), 'gas ceiling changed');
  guard(plan.value === '0' && plan.data === transaction(plan.nonce, plan.gas_price_wei).data && plan.irreversible === true && Number.isFinite(Date.parse(plan.created_at)), 'unexpected transaction payload');
}
export function validateCode(proxy, implementation) {
  guard(proxy === `0x363d3d373d3d3d363d73${IMPLEMENTATION.slice(2)}5af43d82803e903d91602b57fd5bf3` && keccak256(proxy) === TOKEN_HASH, 'token minimal proxy differs from its reviewed fixed target');
  guard(implementation && implementation !== '0x' && keccak256(implementation) === IMPLEMENTATION_HASH, 'token implementation runtime changed');
}
export async function inspect(client, manifest, confirmed = false) {
  manifestCore(manifest); guard(await client.getChainId() === 56, 'RPC must be BNB chain 56');
  const head = await client.getBlockNumber(), blockNumber = confirmed ? head - 11n : head;
  const at = { blockNumber }, block = await client.getBlock(at);
  for (const [address, expected] of [[USDT, PINS.usdt_hash], ...['protocol', 'backing', 'economics'].map(k => [PINS[k].address, PINS[k].code_hash])]) {
    const code = await client.getCode({ address, ...at }); guard(code && code !== '0x' && keccak256(code) === expected, 'active deployment code mismatch');
  }
  validateCode(await client.getCode({ address: TOKEN, ...at }), await client.getCode({ address: IMPLEMENTATION, ...at }));
  const view = (address, functionName, abi = ABI) => client.readContract({ address, abi, functionName, ...at });
  guard(eq(await view(PROTOCOL, 'authority'), SOURCE) && eq(await view(PROTOCOL, 'usdt'), USDT), 'authority or USDT wiring changed');
  guard(await view(TOKEN, 'name', erc20Abi) === 'Tabx402' && await view(TOKEN, 'symbol', erc20Abi) === 'TAB' && await view(TOKEN, 'decimals', erc20Abi) === 18, 'token metadata changed');
  const tab = await view(PROTOCOL, 'tabToken'); guard(eq(tab, ZERO) || eq(tab, TOKEN), 'a different official token is already set');
  guard(eq((await client.getBlock(at)).hash, block.hash), 'inspection block was reorganized');
  return { tab, block: blockNumber, blockHash: block.hash };
}
export async function validateSigned(journal, plan) {
  guard(journal.id === ID && journal.plan_hash === digest(plan) && journal.hash === keccak256(journal.raw), 'signed journal differs from the plan');
  const p = parseTransaction(journal.raw), expected = transaction(plan.nonce, plan.gas_price_wei);
  guard(p.chainId === 56 && p.type === 'legacy' && eq(p.to, PROTOCOL) && p.nonce === expected.nonce && p.gas === expected.gas && p.gasPrice === expected.gasPrice && (p.value ?? 0n) === 0n && p.data === expected.data && eq(await recoverTransactionAddress({ serializedTransaction: journal.raw }), SOURCE), 'signed envelope is not the exact activation');
}
export function validateReceipt(receipt, envelope, block, head, plan, hash) {
  guard(eq(receipt.transactionHash, hash) && eq(envelope.hash, hash) && receipt.status === 'success' && receipt.blockNumber === block.number && eq(receipt.blockHash, block.hash) && head >= block.number + 11n, 'activation is not canonically confirmed 12 times');
  const expected = transaction(plan.nonce, plan.gas_price_wei);
  guard(envelope.chainId === 56 && eq(envelope.from, SOURCE) && eq(envelope.to, PROTOCOL) && envelope.nonce === expected.nonce && envelope.input === expected.data && envelope.value === 0n && envelope.gas === expected.gas && envelope.gasPrice === expected.gasPrice && receipt.gasUsed <= GAS_LIMIT && receipt.effectiveGasPrice === expected.gasPrice, 'receipt differs from the exact authorized call');
  const matching = receipt.logs.filter(l => {
    if (l.removed || !eq(l.address, PROTOCOL)) return false;
    try { const e = decodeEventLog({ abi: ABI, data: l.data, topics: l.topics, strict: true }); return e.eventName === 'TabConfigured' && eq(e.args.token, TOKEN); } catch { return false; }
  });
  guard(matching.length === 1, 'exact TabConfigured event is missing');
  return { transaction_hash: hash, block_number: receipt.blockNumber.toString(), block_hash: receipt.blockHash, gas_cost_wei: (receipt.gasUsed * receipt.effectiveGasPrice).toString() };
}
export async function reconcile(client, journal, plan, send) {
  let receipt;
  try { receipt = await client.getTransactionReceipt({ hash: journal.hash }); } catch (e) { if (e.name !== 'TransactionReceiptNotFoundError') throw e; }
  if (!receipt && send) {
    try { guard(eq(await client.sendRawTransaction({ serializedTransaction: journal.raw }), journal.hash), 'broadcast hash mismatch'); } catch { /* Only the durable original hash may be retried. */ }
  }
  if (!receipt || await client.getBlockNumber() < receipt.blockNumber + 11n) receipt = await client.waitForTransactionReceipt({ hash: journal.hash, confirmations: 12, timeout: 55_000 });
  return validateReceipt(receipt, await client.getTransaction({ hash: journal.hash }), await client.getBlock({ blockNumber: receipt.blockNumber }), await client.getBlockNumber(), plan, journal.hash);
}
async function run(mode) {
  guard(['plan', 'simulate', 'execute', 'verify'].includes(mode), 'usage: tab-token-activate.mjs plan|simulate|execute|verify');
  const client = createPublicClient({ chain: bsc, transport: http(process.env.TAB_BNB_RPC || 'https://bsc-dataseed.bnbchain.org', { timeout: 15_000, retryCount: 0 }) });
  const manifest = read(files.manifest), observed = await inspect(client, manifest);
  if (mode === 'plan' || mode === 'simulate') {
    guard(eq(observed.tab, ZERO), 'token already configured; use verify without another transaction');
    const nonce = await client.getTransactionCount({ address: SOURCE, blockTag: 'latest' });
    guard(nonce === await client.getTransactionCount({ address: SOURCE, blockTag: 'pending' }), 'source has pending transactions');
    const plan = buildPlan(manifest, nonce, await client.getGasPrice(), observed.block);
    const estimated = await client.estimateGas({ ...transaction(nonce, plan.gas_price_wei), account: SOURCE });
    guard(estimated <= GAS_LIMIT && await client.getBalance({ address: SOURCE }) >= GAS_LIMIT * BigInt(plan.gas_price_wei), 'gas estimate or wallet balance exceeds limits');
    await client.call({ ...transaction(nonce, plan.gas_price_wei), account: SOURCE });
    if (mode === 'plan') { guard(!fs.existsSync(files.plan) && !fs.existsSync(files.journal) && !fs.existsSync(files.proof), 'activation records already exist; reconcile them'); write(files.plan, plan); }
    console.log(JSON.stringify({ status: mode === 'plan' ? 'planned' : 'simulated', ...plan, estimated_gas: estimated.toString(), maximum_gas_bnb: formatEther(GAS_LIMIT * BigInt(plan.gas_price_wei)) }, null, 2)); return;
  }
  const plan = fs.existsSync(files.plan) ? read(files.plan) : null;
  if (plan) validatePlan(plan, manifest);
  let journal = fs.existsSync(files.journal) ? read(files.journal) : null;
  if (journal) { guard(plan && (fs.statSync(files.journal).mode & 0o077) === 0, 'journal missing its plan or private permissions'); await validateSigned(journal, plan); }
  let confirmation = null;
  if (!journal && eq(observed.tab, ZERO)) {
    guard(mode === 'execute' && plan && !fs.existsSync(files.proof), 'no signed activation exists to verify');
    const age = Date.now() - Date.parse(plan.created_at); guard(age >= 0 && age <= 30 * 60_000, 'unsigned activation plan expired');
    guard(await client.getTransactionCount({ address: SOURCE, blockTag: 'latest' }) === plan.nonce && await client.getTransactionCount({ address: SOURCE, blockTag: 'pending' }) === plan.nonce, 'source nonce changed; review the existing plan');
    const request = transaction(plan.nonce, plan.gas_price_wei);
    guard(await client.estimateGas({ ...request, account: SOURCE }) <= GAS_LIMIT && await client.getBalance({ address: SOURCE }) >= request.gas * request.gasPrice, 'gas or balance changed');
    await client.call({ ...request, account: SOURCE });
    const key = process.env.TAB_DEPLOYER_KEY; delete process.env.TAB_DEPLOYER_KEY;
    guard(Boolean(key), 'inject the deployer alias through Ryan Vault');
    const account = privateKeyToAccount(key.startsWith('0x') ? key : `0x${key}`); guard(eq(account.address, SOURCE), 'wrong signing wallet');
    const raw = await account.signTransaction(request); journal = { id: ID, plan_hash: digest(plan), raw, hash: keccak256(raw), signed_at: new Date().toISOString() };
    await validateSigned(journal, plan); write(files.journal, journal, 0o600);
  }
  if (journal) { confirmation = await reconcile(client, journal, plan, mode === 'execute' && eq(observed.tab, ZERO)); journal.confirmation = confirmation; write(files.journal, journal, 0o600); }
  const final = await inspect(client, manifest, true); guard(eq(final.tab, TOKEN), 'official token is not set in confirmed chain state');
  const proof = { id: ID, status: 'configured', chain_id: 56, protocol: PROTOCOL, ...TOKEN_FIELDS, ...(confirmation || { transaction_hash: null, proof_source: 'existing confirmed state; no transaction sent by this invocation' }), observed_block: final.block.toString(), observed_block_hash: final.blockHash, verified_at: new Date().toISOString() };
  write(files.proof, proof);
  const current = read(files.manifest); guard(digest(manifestCore(current)) === digest(manifestCore(manifest)), 'active deployment changed before manifest update');
  write(files.manifest, { ...current, ...TOKEN_FIELDS, ...(confirmation ? { official_tab_configuration_transaction: confirmation.transaction_hash, official_tab_configuration_block: Number(confirmation.block_number) } : {}) });
  console.log(JSON.stringify(proof, null, 2));
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[3] !== '--locked') {
    fs.mkdirSync(directory, { recursive: true, mode: 0o700 }); fs.chmodSync(directory, 0o700);
    const result = spawnSync('flock', ['-n', path.join(directory, 'activation.lock'), process.execPath, fileURLToPath(import.meta.url), process.argv[2] || '', '--locked'], { stdio: 'inherit' }); process.exitCode = result.status ?? 1;
  } else run(process.argv[2]).catch(e => { console.error(e.message?.startsWith('TAB activation:') || e.message?.startsWith('Finance pilot:') ? e.message : 'TAB activation: RPC or signing failed. Preserve the private journal and reconcile its exact transaction hash.'); process.exitCode = 1; });
}
