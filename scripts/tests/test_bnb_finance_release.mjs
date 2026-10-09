import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { parseAbi, encodeEventTopics, encodeAbiParameters, decodeFunctionData, keccak256 } from '../../frontend/node_modules/viem/_esm/index.js';
import { privateKeyToAccount } from '../../frontend/node_modules/viem/_esm/accounts/index.js';
import { validateConfig, buildPlan, validatePlan, expectedTransactions, sourceMatches, sourceHash, transaction, validateSigned, validateReceipt, reconcile, financeConfig, inspectFeed, verifyModules, verifyCanonicalProof, WBNB, ID, GAS_CAP, DEBT_CAP, BACKING_ABI } from '../bnb-finance-release.mjs';
import { SOURCE, PROTOCOL, USDT, PINS, digest } from '../finance-pilot.mjs';
import { TOKEN, TOKEN_HASH } from '../tab-token-activate.mjs';

const manifest = JSON.parse(fs.readFileSync(new URL('../../contracts/deployments/bnb-56.json', import.meta.url)));
const hash = `0x${'12'.repeat(32)}`, blockHash = `0x${'34'.repeat(32)}`;
const feed = (prefix, address, maxAge) => ({ [`${prefix}_feed`]: address, [`${prefix}_feed_code_hash`]: hash, [`${prefix}_feed_aggregator`]: `0x${(prefix === 'usdt' ? 'a3' : 'b4').repeat(20)}`, [`${prefix}_feed_aggregator_code_hash`]: hash, [`${prefix}_feed_decimals`]: 8, [`${prefix}_max_age`]: maxAge });
function config() { return { chain_id: 56, modules: { lending: { enabled: true, ...feed('collateral', `0x${'56'.repeat(20)}`, 300), ...feed('usdt', `0x${'78'.repeat(20)}`, 1800), wbnb_code_hash: hash, configure_direct_backing: true }, stock_loans: { enabled: true, ...feed('usdt', `0x${'78'.repeat(20)}`, 1800), assets: [] }, buyback: { enabled: false } } }; }
const constructor = types => ({ type: 'constructor', stateMutability: 'nonpayable', inputs: types.map((type, i) => ({ name: `p${i}`, type })) });
function artifacts() {
  return Object.fromEntries(Object.entries({ oracle: ['address', 'address', 'address', 'address', 'uint32', 'uint32'], lending: ['address', 'address'], stock_loans: ['address', 'address', 'uint32'], buyback: ['address', 'address', 'address[]', 'uint16'] }).map(([key, types]) => [key, { abi: [constructor(types)], bytecode: { object: '0x60006000' }, deployedBytecode: { object: '0x600060006000', immutableReferences: { x: [{ start: 2, length: 2 }] } } }]));
}
const snapshot = () => ({ nonce: 20, pendingNonce: 20, gasPrice: 50_000_000n, balance: GAS_CAP, block: 126600000n, blockHash });
const plan = () => buildPlan(manifest, config(), artifacts(), snapshot(), new Date('2026-10-09T04:05:00Z'));

test('plan only deploys source constructors and explicit immutable WBNB risk policy', () => {
  const p = plan(); validatePlan(p, manifest, config(), artifacts());
  assert.deepEqual(p.transactions.map(x => x.key), ['oracle', 'lending', 'stock_loans', 'direct_backing']);
  assert.deepEqual(p.transactions.map(x => x.nonce), [20, 21, 22, 23]);
  assert.equal(p.maximum_total_gas_wei, GAS_CAP.toString());
  assert.ok(BigInt(p.maximum_cost_wei) < GAS_CAP);
  assert.ok(p.transactions.every(x => x.value === '0'));
  assert.ok(p.transactions.slice(0, -1).every(x => x.to === null));
  const direct = p.transactions.at(-1), data = decodeFunctionData({ abi: BACKING_ABI, data: direct.data });
  assert.equal(direct.to, PINS.backing.address);
  assert.equal(data.functionName, 'configureCollateral');
  assert.deepEqual(data.args.map(x => typeof x === 'string' ? x.toLowerCase() : x), [WBNB, p.transactions[0].address, 5000, 7500, 500, DEBT_CAP]);
  assert.equal(expectedTransactions(config(), artifacts(), 20, 50_000_000n)[1].address, p.transactions[1].address);
});

test('configuration requires reviewed live dependencies and an empty stock whitelist', () => {
  for (const mutate of [c => c.chain_id = 1, c => c.funding = '80', c => c.modules.lending.enabled = 'true', c => c.modules.lending.calldata = '0x', c => c.modules.lending.wbnb_code_hash = null, c => c.modules.lending.collateral_max_age = 172801, c => c.modules.lending.usdt_max_age = 3601, c => c.modules.lending.usdt_feed_aggregator_code_hash = null, c => c.modules.lending.collateral_feed_decimals = 19, c => c.modules.stock_loans.assets.push({ token: TOKEN }), c => c.modules.buyback.router = TOKEN, c => c.modules.lending.configure_direct_backing = undefined]) {
    const c = config(); mutate(c); assert.throws(() => validateConfig(c), /Finance deployment:/);
  }
  const c = config(); c.modules.lending = { enabled: false }; c.modules.stock_loans = { enabled: false };
  assert.throws(() => validateConfig(c), /no modules selected/);
});

test('buyback requires an explicit pinned route to the official token and bounded slippage', () => {
  const c = config(); c.modules.buyback = { enabled: true, router: `0x${'9a'.repeat(20)}`, router_code_hash: hash, route: [USDT, WBNB, TOKEN], route_code_hashes: [PINS.usdt_hash, hash, TOKEN_HASH], maximum_slippage_bps: 300 };
  validateConfig(c); const p = buildPlan(manifest, c, artifacts(), snapshot());
  assert.deepEqual(p.transactions.map(x => x.key), ['oracle', 'lending', 'stock_loans', 'buyback', 'direct_backing']);
  for (const mutate of [x => x.modules.buyback.route[0] = TOKEN, x => x.modules.buyback.route[2] = WBNB, x => x.modules.buyback.route_code_hashes[2] = hash, x => x.modules.buyback.maximum_slippage_bps = 1001, x => x.modules.buyback.route_code_hashes.pop()]) {
    const changed = structuredClone(c); mutate(changed); assert.throws(() => validateConfig(changed));
  }
});

test('nonce, gas ceiling, exact constructors, source and fixed token implementation are bound', () => {
  for (const mutate of [p => p.chain_id = 1, p => p.from = TOKEN, p => p.protocol = TOKEN, p => p.official_tab_implementation_address = SOURCE, p => p.nonce = -1, p => p.transactions[1].nonce++, p => p.transactions[0].value = '1', p => p.transactions[0].gas = '1', p => p.transactions[0].data += '00', p => p.transactions[0].address = SOURCE, p => p.transactions[0].to = PROTOCOL, p => p.maximum_total_gas_wei = '1', p => p.maximum_cost_wei = '1', p => p.gas_price_wei = '1000000001', p => p.source_hash = hash, p => p.manifest_hash = hash, p => p.observed_block_hash = '0x']) {
    const p = plan(); mutate(p); assert.throws(() => validatePlan(p, manifest, config(), artifacts()));
  }
  const m = structuredClone(manifest); m.official_tab_implementation_code_hash = hash;
  assert.throws(() => validatePlan(plan(), m, config(), artifacts()));
  const pending = snapshot(); pending.pendingNonce++;
  assert.throws(() => buildPlan(manifest, config(), artifacts(), pending), /pending transactions/);
  const poor = snapshot(); poor.balance = 0n;
  assert.throws(() => buildPlan(manifest, config(), artifacts(), poor), /cannot cover/);
  const expensive = snapshot(); expensive.gasPrice = 1_000_000_000n;
  assert.throws(() => buildPlan(manifest, config(), artifacts(), expensive), /ceiling/);
  const changed = artifacts(); changed.lending.bytecode.object = '0x60016000';
  assert.notEqual(sourceHash(changed, config()), plan().source_hash);
  assert.throws(() => validatePlan(plan(), manifest, config(), changed));
});

test('runtime comparison masks documented immutable bytes while rejecting changed executable code', () => {
  const a = artifacts().oracle;
  assert.equal(sourceMatches('0x6000ffff6000', a), true);
  assert.equal(sourceMatches('0x6001ffff6000', a), false);
  assert.equal(sourceMatches('0x6000ffff600000', a), false);
  assert.equal(sourceMatches(undefined, a), false);
  const changed = structuredClone(a); changed.deployedBytecode.immutableReferences.x[0].length = 99;
  assert.equal(sourceMatches('0x6000ffff6000', changed), false);
});

function evidence(index = 0) {
  const p = plan(), tx = p.transactions[index], expected = transaction(tx);
  return { p, tx, journal: { hash, raw: '0x1234' }, head: 111n, block: { number: 100n, hash: blockHash }, receipt: { transactionHash: hash, blockNumber: 100n, blockHash, status: 'success', gasUsed: 100_000n, effectiveGasPrice: expected.gasPrice, contractAddress: tx.address, logs: [] }, envelope: { hash, chainId: 56, from: SOURCE, to: tx.to, nonce: tx.nonce, input: tx.data, value: 0n, gas: expected.gas, gasPrice: expected.gasPrice } };
}
const check = e => validateReceipt(e.receipt, e.envelope, e.block, e.head, e.tx, e.journal.hash);
test('canonical twelve-confirmation receipt binds exact constructor, creation address and gas', () => {
  const e = evidence(); assert.equal(check(e).gas_cost_wei, '5000000000000');
  for (const mutate of [x => x.receipt.status = 'reverted', x => x.receipt.contractAddress = SOURCE, x => x.block.hash = hash, x => x.head = 110n, x => x.envelope.from = TOKEN, x => x.envelope.to = PROTOCOL, x => x.envelope.chainId = 1, x => x.envelope.nonce++, x => x.envelope.input += '00', x => x.envelope.gasPrice++, x => x.envelope.value = 1n, x => x.receipt.gasUsed = x.envelope.gas + 1n, x => x.receipt.effectiveGasPrice++]) {
    const changed = evidence(); mutate(changed); assert.throws(() => check(changed), /Finance deployment:/);
  }
});

function directEvidence() {
  const e = evidence(3), oracle = e.p.transactions[0].address;
  e.receipt.logs = [{ address: PINS.backing.address, removed: false, topics: encodeEventTopics({ abi: BACKING_ABI, eventName: 'CollateralConfigured', args: { token: WBNB } }), data: encodeAbiParameters([{ type: 'address' }, { type: 'uint16' }, { type: 'uint16' }, { type: 'uint16' }, { type: 'uint256' }], [oracle, 5000, 7500, 500, DEBT_CAP]) }];
  return e;
}
test('direct backing receipt verifies exact indexed token event and all immutable risk terms', () => {
  assert.equal(check(directEvidence()).key, 'direct_backing');
  for (const mutate of [e => e.receipt.logs = [], e => e.receipt.logs.push(e.receipt.logs[0]), e => e.receipt.logs[0].removed = true, e => e.receipt.logs[0].address = PROTOCOL, e => e.receipt.logs[0].topics[1] = `0x${TOKEN.slice(2).padStart(64, '0')}`, e => e.receipt.logs[0].data = '0x']) {
    const e = directEvidence(); mutate(e); assert.throws(() => check(e), /event is missing/);
  }
});

function client(e, found, uncertain = false) {
  const sent = []; return { sent,
    getTransactionReceipt: async () => { if (found) return e.receipt; const error = new Error(); error.name = 'TransactionReceiptNotFoundError'; throw error; },
    sendRawTransaction: async ({ serializedTransaction }) => { sent.push(serializedTransaction); if (uncertain) throw Error('timeout'); return hash; },
    waitForTransactionReceipt: async () => e.receipt, getBlockNumber: async () => e.head,
    getTransaction: async () => e.envelope, getBlock: async () => e.block,
    getTransactionCount: async () => e.tx.nonce,
  };
}
test('confirmed retries and verify mode never broadcast or create replacement transactions', async () => {
  const e = evidence(), c = client(e, true);
  await reconcile(c, e.journal, e.tx, true); await reconcile(c, e.journal, e.tx, true); assert.deepEqual(c.sent, []);
  const pending = client(e, false); await reconcile(pending, e.journal, e.tx, false); assert.deepEqual(pending.sent, []);
  e.receipt.status = 'reverted'; const failed = client(e, true);
  await assert.rejects(reconcile(failed, e.journal, e.tx, true)); assert.deepEqual(failed.sent, []);
});
test('uncertain broadcast retries only original envelope and nonce conflicts stop safely', async () => {
  const e = evidence(), c = client(e, false, true);
  await reconcile(c, e.journal, e.tx, true); assert.deepEqual(c.sent, ['0x1234']);
  const consumed = client(e, false); consumed.getTransactionCount = async () => e.tx.nonce + 1;
  await assert.rejects(reconcile(consumed, e.journal, e.tx, true), /nonce was consumed/); assert.deepEqual(consumed.sent, []);
  const wrongHash = client(e, false); wrongHash.sendRawTransaction = async () => blockHash;
  await assert.rejects(reconcile(wrongHash, e.journal, e.tx, true), /broadcast hash mismatch/);
});
test('an unrelated public test signer and mutated signed journal are rejected', async () => {
  const p = plan(), tx = p.transactions[0], account = privateKeyToAccount(`0x${'0'.repeat(63)}1`), raw = await account.signTransaction(transaction(tx));
  const journal = { id: ID, plan_hash: digest(p), key: tx.key, raw, hash: keccak256(raw) };
  await assert.rejects(validateSigned(journal, p, tx), /exact constructor or authority/);
  await assert.rejects(validateSigned({ ...journal, plan_hash: hash }, p, tx), /journal differs/);
});
test('backend output preserves empty stock whitelist and never invents buyback readiness', () => {
  const p = plan(), verified = { modules: Object.fromEntries(p.transactions.filter(t => t.address).map(t => [t.key, { address: t.address, code_hash: hash }])) };
  const output = financeConfig(config(), verified);
  assert.equal(output.modules.lending.collateral_feed, config().modules.lending.collateral_feed);
  assert.equal(output.modules.lending.collateral_oracle, verified.modules.oracle.address);
  assert.deepEqual(output.modules.lending.collateral_feed_pins, { aggregator: config().modules.lending.collateral_feed_aggregator, aggregator_code_hash: hash });
  assert.deepEqual(output.modules.stock_loans.usdt_feed_pins, { aggregator: config().modules.stock_loans.usdt_feed_aggregator, aggregator_code_hash: hash });
  assert.deepEqual(output.modules.stock_loans.assets, []);
  assert.equal(output.modules.buyback.address, null);
  assert.deepEqual(output.modules.buyback.route, []);
});

test('generated module and nested proxy pin fields match strict Rust deserialization schemas', () => {
  const p = plan(), verified = { modules: Object.fromEntries(p.transactions.filter(t => t.address).map(t => [t.key, { address: t.address, code_hash: hash }])) };
  const output = financeConfig(config(), verified), source = fs.readFileSync(new URL('../../backend/src/finance.rs', import.meta.url), 'utf8');
  const fields = name => new Set([...source.match(new RegExp(`struct ${name} \\{([\\s\\S]*?)\\n\\}`))[1].matchAll(/^\s+(\w+):/gm)].map(match => match[1]));
  const moduleFields = fields('ModuleConfig'), pinFields = fields('CodePins');
  for (const module of Object.values(output.modules)) for (const [name, value] of Object.entries(module)) {
    assert.ok(moduleFields.has(name), `unknown Rust module field ${name}`);
    if (name.endsWith('_pins')) for (const field of Object.keys(value)) assert.ok(pinFields.has(field), `unknown Rust proxy pin field ${field}`);
  }
  for (const name of ['collateral_feed', 'collateral_feed_code_hash', 'collateral_feed_pins', 'collateral_feed_decimals', 'collateral_max_age', 'usdt_feed', 'usdt_feed_code_hash', 'usdt_feed_pins', 'usdt_feed_decimals', 'usdt_max_age']) assert.notEqual(output.modules.lending[name], undefined);
});

test('feed inspection rejects proxy upgrades, changed aggregator code, precision and stale or incomplete rounds', async () => {
  const proxyCode = '0x60006000', aggregatorCode = '0x60016001', m = config().modules.lending;
  m.collateral_feed_code_hash = keccak256(proxyCode); m.collateral_feed_aggregator_code_hash = keccak256(aggregatorCode);
  const fixture = () => ({ aggregator: m.collateral_feed_aggregator, proxyCode, aggregatorCode, decimals: 8, round: [4n, 600_00000000n, 900n, 995n, 4n] });
  const c = state => ({ getCode: async ({ address }) => address === m.collateral_feed ? state.proxyCode : state.aggregatorCode, readContract: async ({ functionName }) => functionName === 'aggregator' ? state.aggregator : functionName === 'decimals' ? state.decimals : state.round });
  await inspectFeed(c(fixture()), m, 'collateral', { blockNumber: 100n }, 1000n);
  for (const mutate of [x => x.aggregator = SOURCE, x => x.aggregatorCode = '0x6002', x => x.proxyCode = '0x6003', x => x.decimals = 18, x => x.round[0] = 0n, x => x.round[1] = 0n, x => x.round[3] = 600n, x => x.round[3] = 1001n, x => x.round[4] = 3n]) {
    const state = fixture(); mutate(state); await assert.rejects(inspectFeed(c(state), m, 'collateral', { blockNumber: 100n }, 1000n), /Finance deployment:/);
  }
});

test('module verification rejects each changed secured immutable getter and an observation reorg', async () => {
  const c = config(); c.modules.stock_loans = { enabled: false }; c.modules.lending.configure_direct_backing = false;
  const a = artifacts(), p = buildPlan(manifest, c, a, snapshot()), oracle = p.transactions[0].address;
  const values = { token: WBNB, quoteToken: USDT, collateralFeed: c.modules.lending.collateral_feed, quoteFeed: c.modules.lending.usdt_feed, collateralMaxAge: 300, quoteMaxAge: 1800, price: 600n * 10n ** 18n, protocol: PROTOCOL, usdt: USDT, underwriter: SOURCE, asset: USDT, holderGateVersion: 1n, collateralOracle: oracle, WBNB, securedCreditVersion: 1n, LTV_BPS: 5000, LIQUIDATION_BPS: 7500, LIQUIDATION_BONUS_BPS: 500 };
  const fake = (v, block = blockHash) => ({ getCode: async () => '0x6000ffff6000', readContract: async ({ functionName }) => v[functionName], getBlock: async () => ({ hash: block }) });
  const inspected = async () => ({ block: 100n, blockHash });
  await verifyModules(fake(values), manifest, c, a, p, inspected);
  for (const [key, value] of [['token', USDT], ['quoteToken', TOKEN], ['collateralFeed', SOURCE], ['quoteFeed', SOURCE], ['collateralMaxAge', 301], ['quoteMaxAge', 1801], ['price', 0n], ['protocol', SOURCE], ['usdt', TOKEN], ['underwriter', TOKEN], ['asset', TOKEN], ['holderGateVersion', 0n], ['collateralOracle', SOURCE], ['WBNB', USDT], ['securedCreditVersion', 0n], ['LTV_BPS', 6000n], ['LIQUIDATION_BPS', 8000n], ['LIQUIDATION_BONUS_BPS', 600n]]) {
    await assert.rejects(verifyModules(fake({ ...values, [key]: value }), manifest, c, a, p, inspected), /Finance deployment:/);
  }
  await assert.rejects(verifyModules(fake(values, hash), manifest, c, a, p, inspected), /verification block was reorganized/);
});

test('final proof rechecks every canonical receipt and final module observation after getters', async () => {
  const verified = { observed_block: '100', observed_block_hash: blockHash }, confirmations = [{ block_number: '95', block_hash: hash }, { block_number: '96', block_hash: hash }];
  const c = { getBlockNumber: async () => 112n, getBlock: async ({ blockNumber }) => ({ hash: blockNumber === 100n ? blockHash : hash }) };
  await verifyCanonicalProof(c, verified, confirmations);
  await assert.rejects(verifyCanonicalProof({ ...c, getBlockNumber: async () => 106n }, verified, confirmations), /receipt was reorganized/);
  await assert.rejects(verifyCanonicalProof({ ...c, getBlock: async () => ({ hash: blockHash }) }, verified, confirmations), /receipt was reorganized/);
  await assert.rejects(verifyCanonicalProof(c, { ...verified, observed_block_hash: hash }, confirmations), /observation was reorganized/);
});
