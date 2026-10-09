#!/usr/bin/env node
// Fixed first stage of the approved 100 USDT / 0.001 BNB pilot. This moves only
// 10 USDT into each existing agent's withdrawable spending balance. The other
// 80 USDT remains unallocated in the source wallet; no lending pool is deployed.
// Usage: node scripts/finance-pilot.mjs plan|execute|verify
// Only execute consumes TAB_DEPLOYER_KEY, injected through Ryan Vault. The
// signed envelope is fsynced to a private journal BEFORE any broadcast. Keep
// that journal: deleting it is not a supported way to repeat this pilot.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import {
  createPublicClient, http, parseAbi, erc20Abi, encodeFunctionData, decodeEventLog,
  keccak256, toHex, parseTransaction, recoverTransactionAddress, formatEther,
} from '../frontend/node_modules/viem/_esm/index.js';
import { privateKeyToAccount } from '../frontend/node_modules/viem/_esm/accounts/index.js';
import { bsc } from '../frontend/node_modules/viem/_esm/chains/index.js';

export const SOURCE = '0x00e128e7779ea927a087b40ade268dedc1b34a90';
export const USDT = '0x55d398326f99059ff775485246999027b3197955';
export const PROTOCOL = '0xbe2c140c0b40d25ef5531d93c0696318319e6375';
export const UNIT = 10n ** 18n;
export const GAS_CAP = 10n ** 15n; // Aggregate cap for the entire approved pilot.
export const AGENTS = Object.freeze([
  { key: 'finch', id: `${SOURCE}0c6a4b0bb186f1cb77aa9aee`, name: 'tab demo finch', policyHash: '0x5817f84ddc31eab6cec283cdfc19889679cf9c7ffb31e04b71a0e40116370f93' },
  { key: 'wren', id: `${SOURCE}424648c0b9e033324821614c`, name: 'tab demo wren', policyHash: '0x1768b3646da1b407db51fa4b4578382a27ce2dc2500bbca3b07323ad8d930f50' },
]);
export const PINS = Object.freeze({
  usdt_hash: '0x97a48aa4c129657440dafdacd4c836389734d28cc4a0ca7403e68da660a74a59',
  source_hash: '0xf1cc35e6f52f7356baa37f8e70ef1002ff32874ceae62a79ef590d3ea2835bd0',
  protocol: { address: PROTOCOL, code_hash: '0x808e42b1b0d20ec8a87c2b5f7061c0f50370582f98f69baa43ebc276273186c1' },
  backing: { address: '0xfe0d8f539899d095140ec4d56aab46c6293698ff', code_hash: '0x295860397ceb470d9c3c68a6e1163fbc8e8072e2b341d028157173a867acd9c9' },
  economics: { address: '0x541fbdbe2f7b9dd7d0bfa1fa527f7f7732ef9839', code_hash: '0xf5d6c29b774d9b7a5f19ec16cd7ce376462a0bd6abf2d06ea3b8e532f83a6da7' },
});
export const ABI = parseAbi([
  'function authority() view returns (address)',
  'function usdt() view returns (address)',
  'function backing() view returns (address)',
  'function economics() view returns (address)',
  'function protocol() view returns (address)',
  'function tabToken() view returns (address)',
  'function getAgent(bytes32) view returns ((address owner, string name, uint256 dailyCap, bytes32 policyHash, bool paused, uint32 version, uint256 dailySpent, uint64 spendDay))',
  'function getSpending(bytes32) view returns ((uint256 available, uint256 totalFunded, uint256 totalSpent, uint256 totalWithdrawn))',
  'function fundSpending(bytes32 id, uint256 amount)',
  'function withdrawSpending(bytes32 id, uint256 amount)',
  'event SpendingFunded(bytes32 indexed agent, address indexed funder, uint256 amount)',
]);
const ZERO = '0x0000000000000000000000000000000000000000';
const PILOT = 'tab-bnb56-finance-pilot-100-usdt-v1';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const stateDirectory = '/home/ubuntu/.local/state/tabagents/finance-pilot-56';
const files = {
  manifest: path.join(root, 'contracts/deployments/bnb-56.json'),
  plan: path.join(root, 'contracts/deployments/bnb-finance-pilot-56.json'),
  proof: path.join(root, 'contracts/deployments/bnb-finance-pilot-56-receipt.json'),
  journal: path.join(stateDirectory, 'signed-transactions.json'),
};
const equal = (a, b) => typeof a === 'string' && typeof b === 'string' && a.toLowerCase() === b.toLowerCase();
function guard(ok, message) { if (!ok) throw new Error(`Finance pilot: ${message}`); }
const json = value => JSON.stringify(value, (_, v) => typeof v === 'bigint' ? v.toString() : v);
const copy = value => JSON.parse(json(value));
export const digest = value => keccak256(toHex(json(value)));
function read(file) { return JSON.parse(fs.readFileSync(file, 'utf8')); }

// Atomic writes survive process interruption. The containing lock serializes
// all CLI modes; a persisted raw transaction is the sole source for retries.
function write(file, value, mode) {
  const temp = `${file}.${process.pid}.tmp`;
  const fd = fs.openSync(temp, 'wx', mode);
  try { fs.writeFileSync(fd, `${JSON.stringify(value, null, 2)}\n`); fs.fsyncSync(fd); }
  finally { fs.closeSync(fd); }
  fs.renameSync(temp, file);
  const dir = fs.openSync(path.dirname(file), 'r');
  try { fs.fsyncSync(dir); } finally { fs.closeSync(dir); }
}

export function validateManifest(manifest) {
  guard(manifest.status === 'deployed' && manifest.chain_id === 56 && equal(manifest.authority, SOURCE), 'wrong active deployment or authority');
  guard(equal(manifest.usdt_address, USDT) && manifest.usdt_decimals === 18 && manifest.usdt_code_hash === PINS.usdt_hash, 'wrong USDT asset');
  guard(manifest.source_hash === PINS.source_hash && manifest.credit_mode === 'collateralized', 'unreviewed deployment source');
  for (const key of ['protocol', 'backing', 'economics']) {
    guard(equal(manifest.contracts?.[key]?.address, PINS[key].address) && manifest.contracts[key].code_hash === PINS[key].code_hash, `wrong active ${key}`);
  }
  for (const a of AGENTS) guard(manifest.migrated_agents?.some(id => equal(id, a.id)), 'agent was not migrated into v2');
}

export function expectedTransactions(nonce, gasPrice) {
  return [
    { key: 'approve', to: USDT, gas: '100000', data: encodeFunctionData({ abi: erc20Abi, functionName: 'approve', args: [PROTOCOL, 20n * UNIT] }) },
    ...AGENTS.map(a => ({ key: a.key, to: PROTOCOL, gas: '180000', data: encodeFunctionData({ abi: ABI, functionName: 'fundSpending', args: [a.id, 10n * UNIT] }) })),
  ].map((tx, i) => ({ ...tx, nonce: nonce + i, value: '0', gas_price_wei: String(gasPrice) }));
}

export function validateAgent(agent, expected) {
  guard(equal(agent.owner, SOURCE) && agent.name === expected.name && equal(agent.policyHash, expected.policyHash), `${expected.key} owner or policy changed`);
  guard(agent.dailyCap === UNIT && agent.paused === false && agent.version === 1, `${expected.key} cap or registration changed`);
}
function validateLedger(s) {
  guard([s.available, s.totalFunded, s.totalSpent, s.totalWithdrawn].every(x => typeof x === 'bigint' && x >= 0n), 'invalid spending ledger');
  guard(s.available + s.totalSpent + s.totalWithdrawn === s.totalFunded, 'spending ledger does not conserve funds');
}

export function buildPlan(manifest, snapshot, now = new Date()) {
  validateManifest(manifest);
  guard(snapshot.nonce === snapshot.pendingNonce, 'funding wallet has pending transactions');
  guard(snapshot.allowance === 0n, 'existing allowance needs reconciliation');
  guard(snapshot.balance >= 100n * UNIT, 'wallet cannot retain the full 80 USDT pool allocation');
  guard(snapshot.bnb >= GAS_CAP, 'wallet cannot cover the approved gas ceiling');
  for (let i = 0; i < AGENTS.length; i++) {
    validateAgent(snapshot.agents[i].agent, AGENTS[i]);
    validateLedger(snapshot.agents[i].spending);
    guard(snapshot.agents[i].spending.totalFunded === 0n, 'initial agent funding already exists; do not create another pilot');
  }
  const plan = {
    pilot: PILOT, stage: 'agent_spending_only', chain_id: 56, from: SOURCE,
    usdt: USDT, protocol: PROTOCOL, manifest_hash: digest(manifest),
    approved_total_usdt: '100', stage_usdt: '20', reserved_pool_usdt: '80',
    pool: { status: 'blocked', address: null, reason: 'A compatible holder-restricted collateralized pool and verified official TAB token are required.' },
    maximum_total_gas_wei: GAS_CAP.toString(), nonce: snapshot.nonce,
    gas_price_wei: snapshot.gasPrice.toString(), transactions: expectedTransactions(snapshot.nonce, snapshot.gasPrice),
    agents: AGENTS.map(a => ({ ...a, amount_usdt: '10' })),
    observed_block: String(snapshot.block), observed_block_hash: snapshot.blockHash,
    initial_spending: snapshot.agents.map(a => copy(a.spending)),
    created_at: now.toISOString(), automatic_topups: false,
    payment_scope: 'Contract spending balances; these do not fund wallet-based x402 Permit2 payments.',
  };
  validatePlan(plan, manifest);
  return plan;
}

export function validatePlan(plan, manifest) {
  validateManifest(manifest);
  guard(plan.pilot === PILOT && plan.stage === 'agent_spending_only' && plan.chain_id === 56 && equal(plan.from, SOURCE) && equal(plan.usdt, USDT) && equal(plan.protocol, PROTOCOL), 'wrong pilot scope');
  guard(plan.manifest_hash === digest(manifest), 'active manifest changed; review the existing plan');
  guard(plan.approved_total_usdt === '100' && plan.stage_usdt === '20' && plan.reserved_pool_usdt === '80' && plan.pool?.status === 'blocked' && plan.pool?.address === null && plan.automatic_topups === false, 'pilot allocations changed');
  guard(plan.maximum_total_gas_wei === GAS_CAP.toString(), 'aggregate gas ceiling changed');
  guard(Number.isSafeInteger(plan.nonce) && plan.nonce >= 0 && plan.nonce <= Number.MAX_SAFE_INTEGER - 3, 'invalid nonce');
  guard(typeof plan.gas_price_wei === 'string' && /^[1-9]\d*$/.test(plan.gas_price_wei) && BigInt(plan.gas_price_wei) <= 1_000_000_000n, 'gas price exceeds 1 gwei');
  guard(json(plan.transactions) === json(expectedTransactions(plan.nonce, plan.gas_price_wei)), 'transaction payload differs from the fixed 20 USDT stage');
  guard(json(plan.agents) === json(AGENTS.map(a => ({ ...a, amount_usdt: '10' }))), 'agent destinations changed');
  guard(json(plan.initial_spending) === json(AGENTS.map(() => ({ available: '0', totalFunded: '0', totalSpent: '0', totalWithdrawn: '0' }))), 'unexpected initial funding');
  guard(Number.isFinite(Date.parse(plan.created_at)), 'invalid plan time');
  guard(plan.transactions.reduce((sum, tx) => sum + BigInt(tx.gas) * BigInt(tx.gas_price_wei), 0n) <= GAS_CAP, 'planned gas exceeds the aggregate ceiling');
}

export async function snapshot(client, manifest) {
  validateManifest(manifest);
  guard(await client.getChainId() === 56, 'RPC is not BNB chain 56');
  const block = await client.getBlock({ blockTag: 'latest' });
  guard(block.number !== null && block.hash, 'missing canonical block');
  const at = { blockNumber: block.number };
  const readContract = (address, functionName, args = [], abi = ABI) => client.readContract({ address, abi, functionName, args, ...at });
  for (const [address, hash] of [[USDT, PINS.usdt_hash], ...['protocol', 'backing', 'economics'].map(key => [PINS[key].address, PINS[key].code_hash])]) {
    const code = await client.getCode({ address, ...at });
    guard(code && code !== '0x' && keccak256(code) === hash, 'deployed bytecode does not match the reviewed manifest');
  }
  guard(await readContract(USDT, 'decimals', [], erc20Abi) === 18, 'USDT precision changed');
  guard(equal(await readContract(PROTOCOL, 'authority'), SOURCE) && equal(await readContract(PROTOCOL, 'usdt'), USDT), 'protocol authority or token mismatch');
  for (const key of ['backing', 'economics']) {
    guard(equal(await readContract(PROTOCOL, key), PINS[key].address), `${key} wiring mismatch`);
    guard(equal(await readContract(PINS[key].address, 'protocol'), PROTOCOL) && equal(await readContract(PINS[key].address, 'usdt'), USDT), `${key} immutable wiring mismatch`);
  }
  guard(equal(await readContract(PROTOCOL, 'tabToken'), manifest.official_tab_address || ZERO), 'official TAB configuration changed');
  const agents = [];
  for (const a of AGENTS) {
    const agent = await readContract(PROTOCOL, 'getAgent', [a.id]);
    const spending = await readContract(PROTOCOL, 'getSpending', [a.id]);
    validateAgent(agent, a); validateLedger(spending); agents.push({ agent, spending });
  }
  const value = {
    block: block.number, blockHash: block.hash, agents,
    nonce: await client.getTransactionCount({ address: SOURCE, blockTag: 'latest' }),
    pendingNonce: await client.getTransactionCount({ address: SOURCE, blockTag: 'pending' }),
    gasPrice: await client.getGasPrice(), bnb: await client.getBalance({ address: SOURCE, ...at }),
    balance: await readContract(USDT, 'balanceOf', [SOURCE], erc20Abi),
    allowance: await readContract(USDT, 'allowance', [SOURCE, PROTOCOL], erc20Abi),
  };
  guard(equal((await client.getBlock({ blockNumber: block.number })).hash, block.hash), 'snapshot block was reorganized');
  return value;
}

function request(tx) {
  return { chainId: 56, type: 'legacy', to: tx.to, value: 0n, data: tx.data, gas: BigInt(tx.gas), gasPrice: BigInt(tx.gas_price_wei), nonce: tx.nonce };
}
export async function validateSigned(entry, tx) {
  guard(entry?.key === tx.key && typeof entry.raw === 'string' && entry.hash === keccak256(entry.raw), 'journal transaction hash mismatch');
  const parsed = parseTransaction(entry.raw);
  guard(parsed.chainId === 56 && parsed.type === 'legacy' && equal(parsed.to, tx.to) && (parsed.value ?? 0n) === 0n && parsed.data === tx.data && parsed.nonce === tx.nonce && parsed.gas === BigInt(tx.gas) && parsed.gasPrice === BigInt(tx.gas_price_wei), 'signed transaction differs from the plan');
  guard(equal(await recoverTransactionAddress({ serializedTransaction: entry.raw }), SOURCE), 'journal transaction has the wrong signer');
}

export function verifyReceipt(receipt, tx, entry, envelope, block, head) {
  guard(equal(receipt.transactionHash, entry.hash) && equal(envelope.hash, entry.hash) && equal(receipt.blockHash, block.hash) && block.number === receipt.blockNumber && head >= receipt.blockNumber + 11n, 'transaction is not canonically confirmed 12 times');
  guard(envelope.chainId === 56 && equal(envelope.from, SOURCE) && equal(envelope.to, tx.to) && envelope.nonce === tx.nonce && envelope.value === 0n && envelope.input === tx.data && envelope.gas === BigInt(tx.gas) && envelope.gasPrice === BigInt(tx.gas_price_wei), 'confirmed envelope differs from the plan');
  guard(receipt.gasUsed <= BigInt(tx.gas) && receipt.effectiveGasPrice === BigInt(tx.gas_price_wei), 'receipt gas exceeds the signed budget');
  guard(receipt.status === 'success', 'transaction reverted; stop and reconcile, never create a replacement');
  const matches = receipt.logs.filter(log => {
    if (log.removed || !equal(log.address, tx.to)) return false;
    try {
      const decoded = decodeEventLog({ abi: tx.key === 'approve' ? erc20Abi : ABI, data: log.data, topics: log.topics, strict: true });
      if (tx.key === 'approve') return decoded.eventName === 'Approval' && equal(decoded.args.owner, SOURCE) && equal(decoded.args.spender, PROTOCOL) && decoded.args.value === 20n * UNIT;
      const agent = AGENTS.find(a => a.key === tx.key);
      return decoded.eventName === 'SpendingFunded' && equal(decoded.args.agent, agent.id) && equal(decoded.args.funder, SOURCE) && decoded.args.amount === 10n * UNIT;
    } catch { return false; }
  });
  guard(matches.length === 1, 'exact funding or approval event is missing');
  return { key: tx.key, hash: entry.hash, block_number: receipt.blockNumber.toString(), block_hash: receipt.blockHash, gas_cost_wei: (receipt.gasUsed * receipt.effectiveGasPrice).toString() };
}

// An ambiguous RPC send only permits reconciliation/rebroadcast of the same
// durable envelope. Even a stored confirmation is rechecked against the chain.
export async function reconcile(client, tx, entry, send) {
  let receipt;
  try { receipt = await client.getTransactionReceipt({ hash: entry.hash }); }
  catch (error) { if (error.name !== 'TransactionReceiptNotFoundError') throw error; }
  if (!receipt && send) {
    try { guard(equal(await client.sendRawTransaction({ serializedTransaction: entry.raw }), entry.hash), 'broadcast hash mismatch'); }
    catch { /* Retain the exact signed envelope on every uncertain send. */ }
  }
  if (!receipt) receipt = await client.waitForTransactionReceipt({ hash: entry.hash, confirmations: 12, timeout: 55_000 });
  if (await client.getBlockNumber() < receipt.blockNumber + 11n) receipt = await client.waitForTransactionReceipt({ hash: entry.hash, confirmations: 12, timeout: 55_000 });
  const [envelope, block, head] = await Promise.all([
    client.getTransaction({ hash: entry.hash }), client.getBlock({ blockNumber: receipt.blockNumber }), client.getBlockNumber(),
  ]);
  return verifyReceipt(receipt, tx, entry, envelope, block, head);
}

export function validateJournal(journal, plan) {
  guard(journal.pilot === PILOT && journal.plan_hash === digest(plan) && Array.isArray(journal.transactions) && journal.transactions.length <= 3, 'private journal does not match this pilot');
  for (let i = 0; i < journal.transactions.length; i++) guard(journal.transactions[i].key === plan.transactions[i].key, 'journal order or transaction count changed');
}

async function run(mode) {
  guard(['plan', 'execute', 'verify'].includes(mode), 'usage: finance-pilot.mjs plan|execute|verify');
  const client = createPublicClient({ chain: bsc, transport: http(process.env.TAB_BNB_RPC || 'https://bsc-dataseed.bnbchain.org', { timeout: 15_000, retryCount: 0 }) });
  const manifest = read(files.manifest);
  if (mode === 'plan') {
    guard(!fs.existsSync(files.plan) && !fs.existsSync(files.journal) && !fs.existsSync(files.proof), 'a pilot already exists; execute or verify its original journal');
    const plan = buildPlan(manifest, await snapshot(client, manifest));
    const first = request(plan.transactions[0]);
    guard(await client.estimateGas({ ...first, account: SOURCE }) <= first.gas, 'approval exceeds the fixed gas limit');
    write(files.plan, plan, 0o644);
    console.log(JSON.stringify({ status: 'planned', plan: files.plan, stage_usdt: '20', pool_usdt_unallocated: '80', maximum_stage_gas_bnb: formatEther(plan.transactions.reduce((s, tx) => s + BigInt(tx.gas) * BigInt(tx.gas_price_wei), 0n)) }));
    return;
  }
  const plan = read(files.plan); validatePlan(plan, manifest);
  let journal;
  if (fs.existsSync(files.journal)) {
    guard((fs.statSync(files.journal).mode & 0o077) === 0, 'private journal permissions are too broad');
    journal = read(files.journal);
  } else {
    guard(mode === 'execute' && !fs.existsSync(files.proof), 'no signed journal; cannot verify or recreate a completed pilot');
    const age = Date.now() - Date.parse(plan.created_at);
    guard(age >= 0 && age <= 30 * 60_000, 'initial plan expired; do not silently regenerate funding');
    journal = { pilot: PILOT, plan_hash: digest(plan), transactions: [] };
    write(files.journal, journal, 0o600);
  }
  validateJournal(journal, plan);
  if (mode === 'execute' && journal.transactions.length === 0) {
    const age = Date.now() - Date.parse(plan.created_at);
    guard(age >= 0 && age <= 30 * 60_000, 'unsigned plan expired; preserve it and review before funding');
  }
  for (let i = 0; i < journal.transactions.length; i++) await validateSigned(journal.transactions[i], plan.transactions[i]);
  await snapshot(client, manifest); // Fresh chain/code/owner verification even on recovery.
  let account;
  const proofs = [];
  for (let i = 0; i < plan.transactions.length; i++) {
    const tx = plan.transactions[i];
    let entry = journal.transactions[i];
    if (!entry) {
      if (mode === 'verify') break;
      const state = await snapshot(client, manifest);
      guard(state.nonce === tx.nonce && state.pendingNonce === tx.nonce, 'wallet nonce changed; reconcile before signing');
      const fundedCount = Math.max(0, i - 1);
      guard(state.allowance === (i === 0 ? 0n : (20n - BigInt(fundedCount) * 10n) * UNIT), 'spending allowance differs from the completed steps');
      for (let a = 0; a < AGENTS.length; a++) guard(state.agents[a].spending.totalFunded === (a < fundedCount ? 10n * UNIT : 0n), 'agent received funding outside this pilot; stop');
      const remainingGas = plan.transactions.slice(i).reduce((s, t) => s + BigInt(t.gas) * BigInt(t.gas_price_wei), 0n);
      const usedGas = proofs.reduce((s, p) => s + BigInt(p.gas_cost_wei), 0n);
      guard(usedGas + remainingGas <= GAS_CAP && state.bnb >= remainingGas, 'aggregate or wallet gas budget exceeded');
      guard(state.balance >= (100n - BigInt(fundedCount) * 10n) * UNIT, 'funding would consume the intended 80 USDT reserve');
      const signedRequest = request(tx);
      guard(await client.estimateGas({ ...signedRequest, account: SOURCE }) <= signedRequest.gas, 'transaction exceeds its fixed gas limit');
      if (!account) {
        const key = process.env.TAB_DEPLOYER_KEY; delete process.env.TAB_DEPLOYER_KEY;
        guard(Boolean(key), 'inject the deployer alias through Ryan Vault to execute');
        account = privateKeyToAccount(key.startsWith('0x') ? key : `0x${key}`);
        guard(equal(account.address, SOURCE), 'wrong signing wallet');
      }
      const raw = await account.signTransaction(signedRequest);
      entry = { key: tx.key, hash: keccak256(raw), raw, signed_at: new Date().toISOString() };
      await validateSigned(entry, tx);
      journal.transactions.push(entry); write(files.journal, journal, 0o600);
    }
    const proof = await reconcile(client, tx, entry, mode === 'execute');
    proofs.push(proof); entry.confirmed = proof; write(files.journal, journal, 0o600);
  }
  const state = await snapshot(client, manifest);
  const complete = proofs.length === 3;
  if (complete) {
    guard(state.allowance === 0n && state.agents.every(a => a.spending.totalFunded >= 10n * UNIT), 'final funding ledger or exhausted allowance differs');
  }
  const gas = proofs.reduce((s, p) => s + BigInt(p.gas_cost_wei), 0n);
  guard(gas <= GAS_CAP, 'aggregate gas ceiling exceeded');
  const proof = {
    pilot: PILOT, status: complete ? 'agent_stage_confirmed_pool_blocked' : 'partially_confirmed', chain_id: 56,
    source: SOURCE, protocol: PROTOCOL, plan_hash: digest(plan), transactions: proofs,
    funded_agent_usdt: String(Math.max(0, proofs.length - 1) * 10), pool_usdt_allocated: '0', pool_usdt_reserved_in_plan: '80',
    pool_reserve_is_escrowed: false, gas_cost_wei: gas.toString(), remaining_pilot_gas_ceiling_wei: (GAS_CAP - gas).toString(),
    source_balance_usdt: formatEther(state.balance), spending: state.agents.map((a, i) => ({ agent: AGENTS[i].id, ...copy(a.spending) })),
    payment_scope: plan.payment_scope, verified_at: new Date().toISOString(),
  };
  write(files.proof, proof, 0o644); console.log(JSON.stringify(proof, null, 2));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  // The child inherits a kernel-held flock. A crashed process cannot leave a
  // stale user-space lock, and parallel CLI invocations never sign concurrently.
  const mode = process.argv[2];
  if (process.argv[3] !== '--locked') {
    fs.mkdirSync(stateDirectory, { recursive: true, mode: 0o700 });
    fs.chmodSync(stateDirectory, 0o700);
    const result = spawnSync('flock', ['-n', path.join(stateDirectory, 'pilot.lock'), process.execPath, fileURLToPath(import.meta.url), mode || '', '--locked'], { stdio: 'inherit' });
    process.exitCode = result.status ?? 1;
  } else {
    run(mode).catch(error => {
      // RPC/signing exceptions can embed calldata or raw envelopes. Never emit
      // their body; public errors here contain only fixed operational messages.
      console.error(error.message?.startsWith('Finance pilot:') ? error.message : 'Finance pilot: RPC or signing failed. Preserve the journal and reconcile its exact hashes before retrying.');
      process.exitCode = 1;
    });
  }
}
