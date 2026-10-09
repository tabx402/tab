import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {
  buildPlan, validatePlan, validateManifest, validateAgent, validateJournal,
  expectedTransactions, verifyReceipt, reconcile, validateSigned, digest,
  AGENTS, ABI, SOURCE, USDT, PROTOCOL, UNIT, GAS_CAP,
} from '../finance-pilot.mjs';
import { encodeEventTopics, encodeAbiParameters, erc20Abi, keccak256 } from '../../frontend/node_modules/viem/_esm/index.js';
import { privateKeyToAccount } from '../../frontend/node_modules/viem/_esm/accounts/index.js';

const manifest = JSON.parse(fs.readFileSync(new URL('../../contracts/deployments/bnb-56.json', import.meta.url)));
const clone = value => structuredClone(value);
const hash = `0x${'12'.repeat(32)}`;
const blockHash = `0x${'34'.repeat(32)}`;
const zeroLedger = () => ({ available: 0n, totalFunded: 0n, totalSpent: 0n, totalWithdrawn: 0n });
function state() {
  return { nonce: 16, pendingNonce: 16, allowance: 0n, balance: 148n * UNIT, bnb: UNIT,
    gasPrice: 50_000_000n, block: 126563993n, blockHash,
    agents: AGENTS.map(a => ({ agent: { ...a, owner: SOURCE, dailyCap: UNIT, paused: false, version: 1 }, spending: zeroLedger() })),
  };
}
const plan = () => buildPlan(manifest, state(), new Date('2026-10-09T02:00:00Z'));

test('pilot contains exactly approve20/fund10/fund10 and leaves80 unallocated', () => {
  const p = plan(); validatePlan(p, manifest);
  assert.deepEqual(p.transactions, expectedTransactions(16, 50_000_000n));
  assert.deepEqual(p.transactions.map(t => [t.key, t.to, t.value]), [['approve', USDT, '0'], ['finch', PROTOCOL, '0'], ['wren', PROTOCOL, '0']]);
  assert.equal(p.stage_usdt, '20'); assert.equal(p.pool.address, null); assert.equal(p.pool.status, 'blocked');
  assert.equal(p.reserved_pool_usdt, '80'); assert.equal(p.maximum_total_gas_wei, GAS_CAP.toString());
  assert.ok(p.payment_scope.includes('do not fund wallet-based x402'));
});

test('deployment pins reject old protocol, code hashes, authority, token and chain mutations', () => {
  for (const mutate of [
    m => m.chain_id = 1, m => m.authority = PROTOCOL, m => m.usdt_decimals = 6,
    m => m.usdt_address = SOURCE, m => m.usdt_code_hash = hash,
    m => m.source_hash = hash, m => m.credit_mode = 'unsecured',
    m => m.contracts.protocol.address = '0x567e7187d477b1a68c3ac3d292ad44a0d5e770c7',
    m => m.contracts.backing.code_hash = hash, m => m.contracts.economics.address = SOURCE,
    m => m.migrated_agents = [],
  ]) { const m = clone(manifest); mutate(m); assert.throws(() => validateManifest(m), /Finance pilot:/); }
});

test('exact plan rejects payload, allocation, recipient, nonce, gas and manifest tampering', () => {
  for (const mutate of [
    p => p.stage_usdt = '100', p => p.pool.address = PROTOCOL, p => p.reserved_pool_usdt = '0',
    p => p.approved_total_usdt = '101', p => p.maximum_total_gas_wei = UNIT.toString(),
    p => p.automatic_topups = true, p => p.from = PROTOCOL,
    p => p.transactions[1].data = p.transactions[2].data,
    p => p.transactions[0].to = PROTOCOL, p => p.transactions[1].nonce++,
    p => p.transactions[0].gas = '100001', p => p.transactions.push(p.transactions[2]),
    p => p.gas_price_wei = '1000000001', p => p.gas_price_wei = '-1', p => p.nonce = -1,
    p => p.transactions[1].value = '1', p => p.agents[0].id = AGENTS[1].id,
    p => p.initial_spending[0].totalFunded = UNIT.toString(), p => p.manifest_hash = hash,
  ]) { const p = plan(); mutate(p); assert.throws(() => validatePlan(p, manifest), /Finance pilot:/); }
});

test('pending source nonce, old allowance, prior funding, insufficient reserve and gas fail before a plan', () => {
  for (const mutate of [
    s => s.pendingNonce++, s => s.allowance = UNIT,
    s => s.balance = 99n * UNIT, s => s.bnb = GAS_CAP - 1n,
    s => s.agents[0].spending = { available: UNIT, totalFunded: UNIT, totalSpent: 0n, totalWithdrawn: 0n },
    s => s.agents[0].spending.available = UNIT,
  ]) { const s = state(); mutate(s); assert.throws(() => buildPlan(manifest, s), /Finance pilot:/); }
});

test('agent ownership, name, policy, daily cap, paused and registration version are pinned', () => {
  for (const mutate of [a => a.owner = PROTOCOL, a => a.name = 'other', a => a.policyHash = hash,
    a => a.dailyCap = 2n * UNIT, a => a.paused = true, a => a.version = 2]) {
    const a = state().agents[0].agent; mutate(a); assert.throws(() => validateAgent(a, AGENTS[0]), /Finance pilot:/);
  }
});

function evidence(index = 1) {
  const tx = plan().transactions[index];
  const eventName = index === 0 ? 'Approval' : 'SpendingFunded';
  const args = index === 0 ? { owner: SOURCE, spender: PROTOCOL } : { agent: AGENTS[index - 1].id, funder: SOURCE };
  const log = { address: tx.to, removed: false,
    topics: encodeEventTopics({ abi: index === 0 ? erc20Abi : ABI, eventName, args }),
    data: encodeAbiParameters([{ type: 'uint256' }], [(index === 0 ? 20n : 10n) * UNIT]),
  };
  return {
    tx, entry: { key: tx.key, hash, raw: '0x1234' },
    receipt: { transactionHash: hash, blockHash, blockNumber: 100n, status: 'success', gasUsed: 80000n, effectiveGasPrice: 50_000_000n, logs: [log] },
    envelope: { hash, chainId: 56, from: SOURCE, to: tx.to, nonce: tx.nonce, value: 0n, input: tx.data, gas: BigInt(tx.gas), gasPrice: 50_000_000n },
    block: { number: 100n, hash: blockHash }, head: 111n,
  };
}
const check = e => verifyReceipt(e.receipt, e.tx, e.entry, e.envelope, e.block, e.head);

test('canonical confirmed approval and both exact agent funding events are accepted', () => {
  for (let i = 0; i < 3; i++) {
    const e = evidence(i), result = check(e);
    assert.equal(result.key, e.tx.key); assert.equal(result.gas_cost_wei, '4000000000000');
  }
});

test('reverts, reorgs, insufficient confirmations, gas, destination and event mutations are rejected', () => {
  for (const mutate of [
    e => e.receipt.status = 'reverted', e => e.block.hash = hash,
    e => e.head = 110n, e => e.envelope.from = PROTOCOL,
    e => e.envelope.nonce++, e => e.envelope.chainId = 1,
    e => e.envelope.input = '0x', e => e.envelope.to = SOURCE,
    e => e.envelope.value = 1n, e => e.envelope.gasPrice++,
    e => e.receipt.gasUsed = 180001n, e => e.receipt.effectiveGasPrice++,
    e => e.receipt.logs = [], e => e.receipt.logs.push(e.receipt.logs[0]),
    e => e.receipt.logs[0].removed = true, e => e.receipt.logs[0].address = SOURCE,
    e => e.receipt.logs[0].data = encodeAbiParameters([{ type: 'uint256' }], [11n * UNIT]),
    e => e.receipt.logs[0].topics = evidence(2).receipt.logs[0].topics,
  ]) { const e = evidence(); mutate(e); assert.throws(() => check(e), /Finance pilot:/); }
});

function fakeClient(e, initial, uncertain = false) {
  const broadcasts = [];
  return { broadcasts,
    getTransactionReceipt: async () => { if (initial) return e.receipt; const err = new Error('pending'); err.name = 'TransactionReceiptNotFoundError'; throw err; },
    sendRawTransaction: async request => { broadcasts.push(request.serializedTransaction); if (uncertain) throw new Error('timeout'); return hash; },
    waitForTransactionReceipt: async () => e.receipt,
    getTransaction: async () => e.envelope, getBlock: async () => e.block, getBlockNumber: async () => e.head,
  };
}

test('confirmed retry reads its exact receipt without another broadcast', async () => {
  const e = evidence(), client = fakeClient(e, true);
  await reconcile(client, e.tx, e.entry, true); await reconcile(client, e.tx, e.entry, true);
  assert.deepEqual(client.broadcasts, []);
});

test('ambiguous send only rebroadcasts the exact pre-recorded bytes and verify never sends', async () => {
  const e = evidence(), client = fakeClient(e, false, true);
  await reconcile(client, e.tx, e.entry, true); await reconcile(client, e.tx, e.entry, true);
  assert.deepEqual(client.broadcasts, [e.entry.raw, e.entry.raw]);
  const readOnly = fakeClient(e, false);
  await reconcile(readOnly, e.tx, e.entry, false); assert.deepEqual(readOnly.broadcasts, []);
});

test('an already reverted transaction stops recovery without resending', async () => {
  const e = evidence(); e.receipt.status = 'reverted'; const client = fakeClient(e, true);
  await assert.rejects(reconcile(client, e.tx, e.entry, true), /reverted/);
  assert.deepEqual(client.broadcasts, []);
});

test('RPC failure does not turn into a missing receipt and a new broadcast', async () => {
  const e = evidence(), client = fakeClient(e, false);
  client.getTransactionReceipt = async () => { throw new Error('provider down'); };
  await assert.rejects(reconcile(client, e.tx, e.entry, true), /provider down/);
  assert.deepEqual(client.broadcasts, []);
});

test('journal is bound to exact pilot plan and sequential transaction keys', () => {
  const p = plan(), j = { pilot: p.pilot, plan_hash: digest(p), transactions: [{ key: 'approve' }, { key: 'finch' }] };
  validateJournal(j, p);
  for (const mutate of [v => v.plan_hash = hash, v => v.pilot = 'new', v => v.transactions.reverse(), v => v.transactions.push({ key: 'finch' })]) {
    const changed = clone(j); mutate(changed); assert.throws(() => validateJournal(changed, p), /Finance pilot:/);
  }
});

test('signed transaction with valid fields but unrelated published dummy key cannot enter journal', async () => {
  // Public test key 1, never a project key or funded wallet.
  const account = privateKeyToAccount(`0x${'0'.repeat(63)}1`), tx = plan().transactions[0];
  const raw = await account.signTransaction({ chainId: 56, type: 'legacy', to: tx.to, nonce: tx.nonce, data: tx.data, value: 0n, gas: BigInt(tx.gas), gasPrice: BigInt(tx.gas_price_wei) });
  await assert.rejects(validateSigned({ key: tx.key, raw, hash: keccak256(raw) }, tx), /wrong signer/);
  await assert.rejects(validateSigned({ key: tx.key, raw, hash }, tx), /hash mismatch/);
});
