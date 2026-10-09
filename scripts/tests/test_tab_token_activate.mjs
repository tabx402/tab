import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { buildPlan, validatePlan, manifestCore, validateCode, validateReceipt, validateSigned, reconcile, transaction, ABI, TOKEN, TOKEN_FIELDS, TOKEN_HASH, IMPLEMENTATION, IMPLEMENTATION_HASH, GAS_CAP, GAS_LIMIT } from '../tab-token-activate.mjs';
import { SOURCE, PROTOCOL, digest } from '../finance-pilot.mjs';
import { encodeEventTopics, encodeAbiParameters, keccak256 } from '../../frontend/node_modules/viem/_esm/index.js';
import { privateKeyToAccount } from '../../frontend/node_modules/viem/_esm/accounts/index.js';
const manifest = JSON.parse(fs.readFileSync(new URL('../../contracts/deployments/bnb-56.json', import.meta.url)));
const clone = value => structuredClone(value);
const hash = `0x${'34'.repeat(32)}`, blockHash = `0x${'56'.repeat(32)}`;
const plan = () => buildPlan(manifest, 19, 50_000_000n, 126568802n, new Date('2026-10-09T04:05:00Z'));

test('fixed configureTab call has no payment, approval, transfer or deployment', () => {
  const p = plan(); validatePlan(p, manifest);
  assert.equal(p.to, PROTOCOL); assert.equal(p.value, '0');
  assert.equal(p.data, `0x59a786aa${TOKEN.slice(2).padStart(64, '0')}`);
  assert.equal(p.gas, '100000'); assert.equal(p.maximum_gas_wei, GAS_CAP.toString());
  assert.equal(GAS_LIMIT * BigInt(p.gas_price_wei), 5_000_000_000_000n);
  assert.equal(p.token.official_tab_implementation_address, IMPLEMENTATION);
  assert.equal(p.token.official_tab_implementation_code_hash, IMPLEMENTATION_HASH);
});

test('exact token manifest can be staged before signing without changing core plan binding', () => {
  const staged = { ...manifest, ...TOKEN_FIELDS };
  assert.deepEqual(manifestCore(staged), manifestCore(manifest));
  validatePlan(plan(), staged);
  for (const mutate of [m => m.official_tab_address = SOURCE, m => m.official_tab_code_hash = hash,
    m => m.official_tab_implementation_address = SOURCE, m => m.official_tab_implementation_code_hash = hash,
    m => m.official_tab_symbol = 'FAKE', m => m.official_tab_decimals = 6,
    m => m.contracts.protocol.address = SOURCE, m => m.chain_id = 1]) {
    const m = clone(staged); mutate(m); assert.throws(() => validatePlan(plan(), m));
  }
});

test('plan rejects wrong asset, chain, signer, nonce, calldata and excess gas', () => {
  for (const mutate of [p => p.token.official_tab_address = SOURCE, p => p.chain_id = 1,
    p => p.from = TOKEN, p => p.to = TOKEN, p => p.nonce = -1, p => p.data = '0x',
    p => p.value = '1', p => p.gas = '200001', p => p.maximum_gas_wei = '200000000000001',
    p => p.gas_price_wei = '1000000001', p => p.core_hash = hash, p => p.irreversible = false]) {
    const p = clone(plan()); mutate(p); assert.throws(() => validatePlan(p, manifest), /TAB activation:/);
  }
});

test('clone runtime must exactly embed the supplied fixed implementation', () => {
  const proxy = `0x363d3d373d3d3d363d73${IMPLEMENTATION.slice(2)}5af43d82803e903d91602b57fd5bf3`;
  assert.equal(keccak256(proxy), TOKEN_HASH);
  assert.throws(() => validateCode(proxy, '0x6000'), /implementation runtime changed/);
  assert.throws(() => validateCode(proxy.replace(IMPLEMENTATION.slice(2), SOURCE.slice(2)), '0x6000'), /minimal proxy/);
  assert.throws(() => validateCode(`${proxy}00`, '0x6000'), /minimal proxy/);
});

function evidence() {
  const p = plan(), tx = transaction(p.nonce, p.gas_price_wei);
  return { p, journal: { hash, raw: '0x1234' }, head: 111n, block: { number: 100n, hash: blockHash },
    receipt: { transactionHash: hash, blockNumber: 100n, blockHash, status: 'success', gasUsed: 51806n, effectiveGasPrice: tx.gasPrice,
      logs: [{ address: PROTOCOL, removed: false, topics: encodeEventTopics({ abi: ABI, eventName: 'TabConfigured' }), data: encodeAbiParameters([{ type: 'address' }], [TOKEN]) }] },
    envelope: { hash, chainId: 56, from: SOURCE, to: PROTOCOL, nonce: tx.nonce, input: tx.data, value: 0n, gas: tx.gas, gasPrice: tx.gasPrice },
  };
}
const check = e => validateReceipt(e.receipt, e.envelope, e.block, e.head, e.p, e.journal.hash);
test('receipt binds exact canonical call and actual non-indexed TabConfigured event', () => {
  const e = evidence(); assert.equal(e.receipt.logs[0].topics.length, 1);
  assert.equal(check(e).gas_cost_wei, '2590300000000');
  for (const mutate of [x => x.receipt.status = 'reverted', x => x.block.hash = hash,
    x => x.head = 110n, x => x.envelope.from = TOKEN, x => x.envelope.nonce++,
    x => x.envelope.input = '0x', x => x.envelope.gasPrice++, x => x.envelope.value = 1n,
    x => x.receipt.gasUsed = 100001n, x => x.receipt.logs = [],
    x => x.receipt.logs.push(x.receipt.logs[0]), x => x.receipt.logs[0].removed = true,
    x => x.receipt.logs[0].address = SOURCE,
    x => x.receipt.logs[0].data = encodeAbiParameters([{ type: 'address' }], [SOURCE])]) {
    const changed = evidence(); mutate(changed); assert.throws(() => check(changed), /TAB activation:/);
  }
});

function client(e, found, uncertain = false) {
  const sent = []; return { sent,
    getTransactionReceipt: async () => { if (found) return e.receipt; const error = new Error(); error.name = 'TransactionReceiptNotFoundError'; throw error; },
    sendRawTransaction: async ({ serializedTransaction }) => { sent.push(serializedTransaction); if (uncertain) throw Error('timeout'); return hash; },
    waitForTransactionReceipt: async () => e.receipt, getBlockNumber: async () => e.head,
    getTransaction: async () => e.envelope, getBlock: async () => e.block,
  };
}
test('confirmed execute retry sends nothing; verify never broadcasts', async () => {
  const e = evidence(), c = client(e, true);
  await reconcile(c, e.journal, e.p, true); await reconcile(c, e.journal, e.p, true); assert.deepEqual(c.sent, []);
  const pending = client(e, false); await reconcile(pending, e.journal, e.p, false); assert.deepEqual(pending.sent, []);
});
test('uncertain sends retry only the durable envelope; reverts never get replacement calls', async () => {
  const e = evidence(), c = client(e, false, true);
  await reconcile(c, e.journal, e.p, true); assert.deepEqual(c.sent, ['0x1234']);
  e.receipt.status = 'reverted'; const failed = client(e, true);
  await assert.rejects(reconcile(failed, e.journal, e.p, true)); assert.deepEqual(failed.sent, []);
});
test('signed activation rejects a valid envelope from an unrelated public test key', async () => {
  const p = plan(), account = privateKeyToAccount(`0x${'0'.repeat(63)}1`);
  const raw = await account.signTransaction(transaction(p.nonce, p.gas_price_wei));
  const journal = { id: p.id, plan_hash: digest(p), raw, hash: keccak256(raw) };
  await assert.rejects(validateSigned(journal, p), /exact activation/);
  await assert.rejects(validateSigned({ ...journal, plan_hash: hash }, p), /journal differs/);
});
