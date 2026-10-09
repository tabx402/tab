#!/usr/bin/env node
// Deploy optional immutable finance and explicitly reviewed WBNB collateral.
// No approvals, deposits, loans, swaps, funding or stock whitelists change.
// A reviewed public plan hash authorizes deploy; Ryan Vault injects its signer.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createPublicClient, http, parseAbi, erc20Abi, encodeDeployData, encodeFunctionData, decodeEventLog, getContractAddress, keccak256, toHex, parseTransaction, recoverTransactionAddress, formatEther } from '../frontend/node_modules/viem/_esm/index.js';
import { privateKeyToAccount } from '../frontend/node_modules/viem/_esm/accounts/index.js';
import { bsc } from '../frontend/node_modules/viem/_esm/chains/index.js';
import { SOURCE, PROTOCOL, USDT, PINS, digest, validateManifest } from './finance-pilot.mjs';
import { TOKEN, TOKEN_FIELDS, TOKEN_HASH, IMPLEMENTATION, IMPLEMENTATION_HASH, inspect as inspectToken } from './tab-token-activate.mjs';

export const WBNB = '0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c';
export const GAS_CAP = 5_000_000_000_000_000n;
export const GAS_LIMITS = Object.freeze({ oracle: 1_500_000n, lending: 7_000_000n, stock_loans: 7_000_000n, buyback: 3_000_000n, direct_backing: 250_000n });
export const NAMES = Object.freeze({ oracle: 'TabPriceOracle', lending: 'TabLendingPool', stock_loans: 'TabStockLending', buyback: 'TabBuyback' });
export const ID = 'tab-bnb56-optional-secured-finance-v1';
export const DEBT_CAP = 10_000n * 10n ** 18n;
export const BACKING_ABI = parseAbi(['function collateralAssets(address) view returns(address oracle,uint16 ltvBps,uint16 liquidationBps,uint16 bonusBps,uint8 decimals,bool paused,uint256 debtCap,uint256 debt)', 'function configureCollateral(address token,address oracle,uint16 ltv,uint16 threshold,uint16 bonus,uint256 cap)', 'event CollateralConfigured(address indexed token,address oracle,uint16 ltvBps,uint16 liquidationBps,uint16 bonusBps,uint256 debtCap)']);
const PROTOCOL_ABI = parseAbi(['function MAX_BUDGET() view returns(uint256)']);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const state = '/home/ubuntu/.local/state/tabagents/finance-deployment-56';
const directory = path.join(root, 'contracts/deployments');
const files = { manifest: path.join(directory, 'bnb-56.json'), input: path.join(directory, 'bnb-finance-deployment-56.json'), plan: path.join(directory, 'bnb-finance-56-plan.json'), proof: path.join(directory, 'bnb-finance-56-receipt.json'), config: path.join(directory, 'bnb-finance-56-config.json'), journal: path.join(state, 'signed-transactions.json') };
const eq = (a, b) => typeof a === 'string' && typeof b === 'string' && a.toLowerCase() === b.toLowerCase();
const json = value => JSON.stringify(value, (_, v) => typeof v === 'bigint' ? v.toString() : v);
const copy = value => JSON.parse(json(value));
function guard(ok, message) { if (!ok) throw new Error(`Finance deployment: ${message}`); }
function read(file) { return JSON.parse(fs.readFileSync(file, 'utf8')); }
function write(file, value, mode = 0o644) {
  const temp = `${file}.${process.pid}.tmp`, fd = fs.openSync(temp, 'wx', mode);
  try { fs.writeFileSync(fd, `${JSON.stringify(value, null, 2)}\n`); fs.fsyncSync(fd); } finally { fs.closeSync(fd); }
  fs.renameSync(temp, file);
  const dir = fs.openSync(path.dirname(file), 'r'); try { fs.fsyncSync(dir); } finally { fs.closeSync(dir); }
}
function keys(value, allowed) { guard(value && typeof value === 'object' && !Array.isArray(value) && Object.keys(value).every(k => allowed.includes(k)), 'unknown configuration fields'); }
function address(value) { guard(typeof value === 'string' && /^0x[0-9a-f]{40}$/.test(value) && !/^0x0{40}$/.test(value), 'addresses must be explicit nonzero lowercase values'); }
function hash(value) { guard(typeof value === 'string' && /^0x[0-9a-f]{64}$/.test(value), 'an exact reviewed runtime hash is required'); }
function age(value, maximum) { guard(Number.isInteger(value) && value >= 60 && value <= maximum, 'feed freshness bounds are invalid'); }
function feed(config, prefix, maximum) {
  address(config[`${prefix}_feed`]); hash(config[`${prefix}_feed_code_hash`]);
  address(config[`${prefix}_feed_aggregator`]); hash(config[`${prefix}_feed_aggregator_code_hash`]);
  guard(Number.isInteger(config[`${prefix}_feed_decimals`]) && config[`${prefix}_feed_decimals`] >= 0 && config[`${prefix}_feed_decimals`] <= 18, 'feed decimals must be reviewed');
  age(config[`${prefix}_max_age`], maximum);
}
export function validateConfig(config) {
  keys(config, ['chain_id', 'modules']); guard(config.chain_id === 56, 'configuration must select chain 56');
  keys(config.modules, ['lending', 'stock_loans', 'buyback']);
  let enabled = 0;
  for (const key of ['lending', 'stock_loans', 'buyback']) {
    const m = config.modules[key]; guard(m && typeof m.enabled === 'boolean', 'each module needs an explicit enabled boolean');
    if (!m.enabled) { keys(m, ['enabled']); continue; }
    enabled++;
    if (key === 'lending') {
      keys(m, ['enabled', 'collateral_feed', 'collateral_feed_code_hash', 'collateral_feed_aggregator', 'collateral_feed_aggregator_code_hash', 'collateral_feed_decimals', 'collateral_max_age', 'usdt_feed', 'usdt_feed_code_hash', 'usdt_feed_aggregator', 'usdt_feed_aggregator_code_hash', 'usdt_feed_decimals', 'usdt_max_age', 'wbnb_code_hash', 'configure_direct_backing']);
      feed(m, 'collateral', 2 * 86400); feed(m, 'usdt', 3600); hash(m.wbnb_code_hash);
      guard(m.usdt_feed !== m.collateral_feed, 'collateral and quote feeds must be distinct');
      guard(typeof m.configure_direct_backing === 'boolean', 'direct backing configuration needs an explicit boolean');
    } else if (key === 'stock_loans') {
      keys(m, ['enabled', 'usdt_feed', 'usdt_feed_code_hash', 'usdt_feed_aggregator', 'usdt_feed_aggregator_code_hash', 'usdt_feed_decimals', 'usdt_max_age', 'assets']);
      feed(m, 'usdt', 86400); guard(Array.isArray(m.assets) && m.assets.length === 0, 'stock collateral whitelist must remain empty');
    } else {
      keys(m, ['enabled', 'router', 'router_code_hash', 'route', 'route_code_hashes', 'maximum_slippage_bps']);
      address(m.router); hash(m.router_code_hash);
      guard(Array.isArray(m.route) && m.route.length >= 2 && m.route.length <= 4 && m.route[0] === USDT && m.route.at(-1) === TOKEN && new Set(m.route).size === m.route.length, 'buyback route must end at the supplied official TAB token');
      m.route.forEach(address); guard(Array.isArray(m.route_code_hashes) && m.route_code_hashes.length === m.route.length, 'all route runtime hashes must be explicit'); m.route_code_hashes.forEach(hash);
      guard(m.route_code_hashes[0] === PINS.usdt_hash && m.route_code_hashes.at(-1) === TOKEN_HASH, 'route asset runtime pins differ');
      guard(Number.isInteger(m.maximum_slippage_bps) && m.maximum_slippage_bps >= 0 && m.maximum_slippage_bps <= 1000, 'slippage must be between 0 and 1000 bps');
    }
  }
  guard(enabled > 0, 'no modules selected'); return config;
}
export function manifestBinding(manifest) {
  validateManifest(manifest);
  for (const [key, expected] of Object.entries(TOKEN_FIELDS)) guard(manifest[key] === expected, 'official token or fixed implementation pins changed');
  guard(manifest.holder_access_enabled === true, 'holder policy must be active');
  return digest(manifest);
}
export function sourceHash(artifacts, config) {
  const selected = ['oracle', 'lending', 'stock_loans', 'buyback'].filter(k => k === 'oracle' ? config.modules.lending.enabled : config.modules[k].enabled);
  return digest(selected.map(k => [k, artifacts[k]?.abi, artifacts[k]?.bytecode, artifacts[k]?.deployedBytecode]));
}
export function sourceMatches(code, artifact) {
  if (!code || !/^0x[0-9a-f]+$/i.test(code)) return false;
  let observed = code.slice(2).toLowerCase(), expected = artifact.deployedBytecode.object.replace(/^0x/, '').toLowerCase();
  if (observed.length !== expected.length) return false;
  for (const ranges of Object.values(artifact.deployedBytecode.immutableReferences || {})) for (const { start, length } of ranges) {
    if (!Number.isInteger(start) || !Number.isInteger(length) || start < 0 || length < 1 || (start + length) * 2 > expected.length) return false;
    const offset = start * 2, size = length * 2;
    observed = observed.slice(0, offset) + '0'.repeat(size) + observed.slice(offset + size);
    expected = expected.slice(0, offset) + '0'.repeat(size) + expected.slice(offset + size);
  }
  return observed === expected;
}
function loadArtifacts(config) {
  const result = {};
  for (const [key, name] of Object.entries(NAMES)) {
    if (!(key === 'oracle' ? config.modules.lending.enabled : config.modules[key].enabled)) continue;
    const artifact = read(path.join(root, `contracts/bnb/out/${name}.sol/${name}.json`));
    guard(/^0x[0-9a-f]+$/i.test(artifact.bytecode?.object), 'compile the selected contract before planning');
    for (const [source, metadata] of Object.entries(artifact.metadata?.sources || {})) {
      const file = path.resolve(root, 'contracts/bnb', source);
      guard(file.startsWith(`${root}/contracts/bnb/`) && fs.existsSync(file) && keccak256(toHex(fs.readFileSync(file))) === metadata.keccak256, 'compiled sources are stale; rebuild contracts');
    }
    guard(Object.keys(artifact.metadata?.sources || {}).length > 0, 'artifact source metadata is missing');
    if (key === 'lending') guard(artifact.abi.some(x => x.type === 'function' && x.name === 'securedCreditVersion'), 'unsecured pool artifacts cannot be deployed');
    result[key] = artifact;
  }
  return result;
}
export function expectedTransactions(config, artifacts, nonce, gasPrice) {
  validateConfig(config); guard(Number.isSafeInteger(nonce) && nonce >= 0 && nonce < Number.MAX_SAFE_INTEGER - 4, 'invalid starting nonce');
  const selected = ['oracle', 'lending', 'stock_loans', 'buyback'].filter(k => k === 'oracle' ? config.modules.lending.enabled : config.modules[k].enabled);
  const addresses = Object.fromEntries(selected.map((key, i) => [key, getContractAddress({ from: SOURCE, nonce: BigInt(nonce + i) }).toLowerCase()]));
  const transactions = selected.map((key, i) => {
    const m = config.modules[key === 'oracle' ? 'lending' : key];
    const args = key === 'oracle' ? [WBNB, USDT, m.collateral_feed, m.usdt_feed, m.collateral_max_age, m.usdt_max_age] : key === 'lending' ? [PROTOCOL, addresses.oracle] : key === 'stock_loans' ? [PROTOCOL, m.usdt_feed, m.usdt_max_age] : [PROTOCOL, m.router, m.route, m.maximum_slippage_bps];
    guard(artifacts[key]?.abi && artifacts[key]?.bytecode?.object, 'selected compiled artifact is missing');
    return { key, nonce: nonce + i, to: null, value: '0', gas: GAS_LIMITS[key].toString(), gas_price_wei: String(gasPrice), address: addresses[key], data: encodeDeployData({ abi: artifacts[key].abi, bytecode: artifacts[key].bytecode.object, args }) };
  });
  if (config.modules.lending.enabled && config.modules.lending.configure_direct_backing) transactions.push({ key: 'direct_backing', nonce: nonce + transactions.length, to: PINS.backing.address, value: '0', gas: GAS_LIMITS.direct_backing.toString(), gas_price_wei: String(gasPrice), address: null, data: encodeFunctionData({ abi: BACKING_ABI, functionName: 'configureCollateral', args: [WBNB, addresses.oracle, 5000, 7500, 500, DEBT_CAP] }) });
  return transactions;
}
export function buildPlan(manifest, config, artifacts, snapshot, now = new Date()) {
  guard(snapshot.nonce === snapshot.pendingNonce, 'deployer has pending transactions');
  const transactions = expectedTransactions(config, artifacts, snapshot.nonce, snapshot.gasPrice);
  const plan = { id: ID, chain_id: 56, from: SOURCE, protocol: PROTOCOL, official_tab_address: TOKEN, official_tab_implementation_address: IMPLEMENTATION, manifest_hash: manifestBinding(manifest), source_hash: sourceHash(artifacts, config), config: copy(config), nonce: snapshot.nonce, gas_price_wei: String(snapshot.gasPrice), maximum_total_gas_wei: GAS_CAP.toString(), maximum_cost_wei: transactions.reduce((s, t) => s + BigInt(t.gas) * BigInt(t.gas_price_wei), 0n).toString(), transactions, observed_block: String(snapshot.block), observed_block_hash: snapshot.blockHash, created_at: now.toISOString(), scope: 'Immutable deployment and explicit WBNB collateral policy only. No funding, approvals, swaps, loans, stock whitelists or production service changes.' };
  validatePlan(plan, manifest, config, artifacts); guard(snapshot.balance >= BigInt(plan.maximum_cost_wei), 'deployer cannot cover planned maximum gas'); return plan;
}
export function validatePlan(plan, manifest, config, artifacts) {
  validateConfig(config);
  guard(plan.id === ID && plan.chain_id === 56 && plan.from === SOURCE && plan.protocol === PROTOCOL && plan.official_tab_address === TOKEN && plan.official_tab_implementation_address === IMPLEMENTATION, 'wrong deployment scope or targets');
  guard(plan.manifest_hash === manifestBinding(manifest) && plan.source_hash === sourceHash(artifacts, config) && json(plan.config) === json(config), 'manifest, source or reviewed configuration changed');
  guard(typeof plan.gas_price_wei === 'string' && /^[1-9]\d*$/.test(plan.gas_price_wei) && BigInt(plan.gas_price_wei) <= 1_000_000_000n && plan.maximum_total_gas_wei === GAS_CAP.toString(), 'gas price or aggregate ceiling changed');
  const expected = expectedTransactions(config, artifacts, plan.nonce, plan.gas_price_wei);
  guard(json(plan.transactions) === json(expected), 'transactions differ from compiled constructors');
  const maximum = expected.reduce((s, t) => s + BigInt(t.gas) * BigInt(t.gas_price_wei), 0n);
  guard(maximum <= GAS_CAP && plan.maximum_cost_wei === maximum.toString(), 'plan exceeds reviewed 0.005 BNB ceiling');
  guard(Number.isFinite(Date.parse(plan.created_at)) && /^\d+$/.test(plan.observed_block), 'invalid observation time or block'); hash(plan.observed_block_hash);
}
export function transaction(tx) { return { chainId: 56, type: 'legacy', ...(tx.to ? { to: tx.to } : {}), value: 0n, data: tx.data, nonce: tx.nonce, gas: BigInt(tx.gas), gasPrice: BigInt(tx.gas_price_wei) }; }
export async function validateSigned(journal, plan, tx) {
  guard(journal.id === ID && journal.plan_hash === digest(plan) && journal.key === tx.key && journal.hash === keccak256(journal.raw), 'private signed journal differs from the plan');
  const p = parseTransaction(journal.raw), expected = transaction(tx);
  guard(p.chainId === 56 && p.type === 'legacy' && (tx.to ? eq(p.to, tx.to) : p.to == null) && p.nonce === expected.nonce && p.gas === expected.gas && p.gasPrice === expected.gasPrice && (p.value ?? 0n) === 0n && p.data === expected.data && eq(await recoverTransactionAddress({ serializedTransaction: journal.raw }), SOURCE), 'signed envelope differs from the exact constructor or authority');
}
export function validateReceipt(receipt, envelope, block, head, tx, hash) {
  guard(eq(receipt.transactionHash, hash) && eq(envelope.hash, hash) && receipt.status === 'success' && receipt.blockNumber === block.number && eq(receipt.blockHash, block.hash) && head >= block.number + 11n, 'deployment is not canonically confirmed 12 times');
  const expected = transaction(tx);
  guard(envelope.chainId === 56 && eq(envelope.from, SOURCE) && (tx.to ? eq(envelope.to, tx.to) : envelope.to == null) && envelope.nonce === expected.nonce && envelope.input === expected.data && envelope.value === 0n && envelope.gas === expected.gas && envelope.gasPrice === expected.gasPrice && receipt.gasUsed <= expected.gas && receipt.effectiveGasPrice === expected.gasPrice && (tx.address ? eq(receipt.contractAddress, tx.address) : receipt.contractAddress == null), 'receipt differs from the exact planned deployment');
  if (tx.key === 'direct_backing') {
    const oracle = `0x${tx.data.slice(10 + 64, 10 + 128).slice(-40)}`;
    const matching = (receipt.logs || []).filter(log => {
      if (log.removed || !eq(log.address, PINS.backing.address)) return false;
      try { const event = decodeEventLog({ abi: BACKING_ABI, data: log.data, topics: log.topics, strict: true }); return event.eventName === 'CollateralConfigured' && eq(event.args.token, WBNB) && eq(event.args.oracle, oracle) && event.args.ltvBps === 5000 && event.args.liquidationBps === 7500 && event.args.bonusBps === 500 && event.args.debtCap === DEBT_CAP; } catch { return false; }
    });
    guard(matching.length === 1, 'exact WBNB collateral policy event is missing');
  }
  return { key: tx.key, address: tx.address, transaction_hash: hash, block_number: receipt.blockNumber.toString(), block_hash: receipt.blockHash, gas_cost_wei: (receipt.gasUsed * receipt.effectiveGasPrice).toString() };
}
export async function reconcile(client, entry, tx, send) {
  let receipt;
  try { receipt = await client.getTransactionReceipt({ hash: entry.hash }); } catch (e) { if (e.name !== 'TransactionReceiptNotFoundError') throw e; }
  if (!receipt && send) {
    const nonce = await client.getTransactionCount({ address: SOURCE, blockTag: 'latest' });
    guard(nonce <= tx.nonce, 'planned nonce was consumed without a matching receipt; reconcile before retry');
    try { const result = await client.sendRawTransaction({ serializedTransaction: entry.raw }); guard(eq(result, entry.hash), 'broadcast hash mismatch'); } catch (e) { if (e.message?.startsWith('Finance deployment:')) throw e; /* Reconcile only this durable transaction. */ }
  }
  if (!receipt || await client.getBlockNumber({ cacheTime: 0 }) < receipt.blockNumber + 11n) receipt = await client.waitForTransactionReceipt({ hash: entry.hash, confirmations: 12, timeout: 55_000 });
  return validateReceipt(receipt, await client.getTransaction({ hash: entry.hash }), await client.getBlock({ blockNumber: receipt.blockNumber }), await client.getBlockNumber({ cacheTime: 0 }), tx, entry.hash);
}
const FEED_ABI = parseAbi(['function decimals() view returns(uint8)', 'function aggregator() view returns(address)', 'function latestRoundData() view returns(uint80,int256,uint256,uint256,uint80)']);
const ROUTER_ABI = parseAbi(['function getAmountsOut(uint256 amount,address[] path) view returns(uint256[])']);
export async function inspectFeed(client, m, prefix, at, timestamp) {
  const address = m[`${prefix}_feed`], runtime = await client.getCode({ address, ...at });
  guard(runtime && runtime !== '0x' && keccak256(runtime) === m[`${prefix}_feed_code_hash`], 'reviewed dependency runtime hash changed');
  const aggregator = await client.readContract({ address, abi: FEED_ABI, functionName: 'aggregator', ...at });
  guard(eq(aggregator, m[`${prefix}_feed_aggregator`]), 'reviewed feed proxy implementation changed');
  const code = await client.getCode({ address: aggregator, ...at });
  guard(code && code !== '0x' && keccak256(code) === m[`${prefix}_feed_aggregator_code_hash`], 'reviewed feed aggregator runtime changed');
  const decimals = await client.readContract({ address, abi: FEED_ABI, functionName: 'decimals', ...at });
  const [round, answer,, updated, answered] = await client.readContract({ address, abi: FEED_ABI, functionName: 'latestRoundData', ...at });
  guard(decimals === m[`${prefix}_feed_decimals`] && round > 0n && answer > 0n && answer <= 10n ** 30n && answered >= round && updated > 0n && updated <= timestamp && timestamp - updated <= BigInt(m[`${prefix}_max_age`]), 'reviewed feed is stale, invalid or has different decimals');
}
export async function inspect(client, manifest, config, confirmed = false) {
  manifestBinding(manifest); validateConfig(config);
  const observed = await inspectToken(client, manifest, confirmed); guard(eq(observed.tab, TOKEN), 'official TAB has not been configured');
  const at = { blockNumber: observed.block }, block = await client.getBlock(at);
  const pinnedCode = async (address, expected) => { const code = await client.getCode({ address, ...at }); guard(code && code !== '0x' && keccak256(code) === expected, 'reviewed dependency runtime hash changed'); };
  for (const m of Object.values(config.modules).filter(x => x.enabled)) for (const prefix of ['collateral', 'usdt']) if (m[`${prefix}_feed`]) {
    await inspectFeed(client, m, prefix, at, block.timestamp);
  }
  if (config.modules.lending.enabled) { await pinnedCode(WBNB, config.modules.lending.wbnb_code_hash); guard(await client.readContract({ address: WBNB, abi: erc20Abi, functionName: 'decimals', ...at }) === 18, 'WBNB decimals changed'); guard(await client.readContract({ address: PROTOCOL, abi: PROTOCOL_ABI, functionName: 'MAX_BUDGET', ...at }) === DEBT_CAP, 'protocol maximum debt budget changed'); }
  if (config.modules.buyback.enabled) {
    const m = config.modules.buyback; await pinnedCode(m.router, m.router_code_hash);
    for (let i = 0; i < m.route.length; i++) await pinnedCode(m.route[i], m.route_code_hashes[i]);
    const amounts = await client.readContract({ address: m.router, abi: ROUTER_ABI, functionName: 'getAmountsOut', args: [10n ** 18n, m.route], ...at });
    guard(amounts.length === m.route.length && amounts[0] === 10n ** 18n && amounts.every(x => x > 0n), 'reviewed TAB buyback route has no usable live quote');
  }
  guard(eq((await client.getBlock(at)).hash, observed.blockHash), 'inspection block was reorganized'); return observed;
}
export async function verifyModules(client, manifest, config, artifacts, plan, inspectDependencies = inspect) {
  const observed = await inspectDependencies(client, manifest, config, true), at = { blockNumber: observed.block }, modules = {};
  for (const tx of plan.transactions) {
    if (tx.key === 'direct_backing') {
      const a = await client.readContract({ address: PINS.backing.address, abi: BACKING_ABI, functionName: 'collateralAssets', args: [WBNB], ...at });
      guard(eq(a[0], plan.transactions.find(t => t.key === 'oracle').address) && a[1] === 5000 && a[2] === 7500 && a[3] === 500 && a[4] === 18 && a[5] === false && a[6] === DEBT_CAP, 'direct WBNB collateral policy differs from reviewed terms');
      continue;
    }
    const address = tx.address, artifact = artifacts[tx.key], code = await client.getCode({ address, ...at });
    guard(sourceMatches(code, artifact), `${tx.key} deployed runtime differs from compiled source`);
    const view = (name, args = []) => client.readContract({ address, abi: artifact.abi, functionName: name, args, ...at });
    if (tx.key === 'oracle') {
      const m = config.modules.lending;
      for (const [name, value] of [['token', WBNB], ['quoteToken', USDT], ['collateralFeed', m.collateral_feed], ['quoteFeed', m.usdt_feed]]) guard(eq(await view(name), value), 'oracle dependency wiring changed');
      guard(Number(await view('collateralMaxAge')) === m.collateral_max_age && Number(await view('quoteMaxAge')) === m.usdt_max_age && await view('price') > 0n, 'oracle freshness or price differs');
    } else {
      guard(eq(await view('protocol'), PROTOCOL) && eq(await view('usdt'), USDT), 'finance module protocol or USDT wiring differs');
      if (tx.key === 'buyback') {
        const m = config.modules.buyback;
        guard(eq(await view('operator'), SOURCE) && eq(await view('token'), TOKEN) && eq(await view('router'), m.router) && Number(await view('maximumSlippageBps')) === m.maximum_slippage_bps && json((await view('getRoute')).map(x => x.toLowerCase())) === json(m.route), 'buyback constructor configuration differs');
      } else {
        guard(eq(await view('underwriter'), SOURCE) && eq(await view('asset'), USDT) && await view('holderGateVersion') === 1n, 'pool authority, asset or holder policy differs');
        if (tx.key === 'lending') {
          guard(eq(await view('collateralOracle'), plan.transactions.find(t => t.key === 'oracle').address) && eq(await view('WBNB'), WBNB) && await view('securedCreditVersion') === 1n && Number(await view('LTV_BPS')) === 5000 && Number(await view('LIQUIDATION_BPS')) === 7500 && Number(await view('LIQUIDATION_BONUS_BPS')) === 500, 'secured pool collateral or risk terms differ');
        } else {
          const m = config.modules.stock_loans;
          guard(eq(await view('usdtUsdFeed'), m.usdt_feed) && Number(await view('usdtMaxAge')) === m.usdt_max_age && Number(await view('usdtFeedDecimals')) === m.usdt_feed_decimals, 'stock pool quote feed differs');
        }
      }
    }
    modules[tx.key] = { address, code_hash: keccak256(code) };
  }
  guard(eq((await client.getBlock(at)).hash, observed.blockHash), 'module verification block was reorganized');
  return { modules, observed_block: observed.block.toString(), observed_block_hash: observed.blockHash };
}
export async function verifyCanonicalProof(client, verified, confirmations) {
  const head = await client.getBlockNumber({ cacheTime: 0 });
  for (const confirmation of confirmations) {
    const number = BigInt(confirmation.block_number), block = await client.getBlock({ blockNumber: number });
    guard(eq(block.hash, confirmation.block_hash) && head >= number + 11n, 'confirmed deployment receipt was reorganized before proof');
  }
  const block = await client.getBlock({ blockNumber: BigInt(verified.observed_block) });
  guard(eq(block.hash, verified.observed_block_hash), 'module proof observation was reorganized');
}
export function financeConfig(config, verified) {
  const modules = { lending: { address: null, code_hash: null }, stock_loans: { address: null, code_hash: null, usdt_feed: null, usdt_feed_code_hash: null, usdt_max_age: null, usdt_feed_decimals: null, assets: [] }, buyback: { address: null, code_hash: null, router: null, router_code_hash: null, official_token_code_hash: null, route: [] } };
  const metadata = (m, prefix) => ({ [`${prefix}_feed`]: m[`${prefix}_feed`], [`${prefix}_feed_code_hash`]: m[`${prefix}_feed_code_hash`], [`${prefix}_feed_pins`]: { aggregator: m[`${prefix}_feed_aggregator`], aggregator_code_hash: m[`${prefix}_feed_aggregator_code_hash`] }, [`${prefix}_feed_decimals`]: m[`${prefix}_feed_decimals`], [`${prefix}_max_age`]: m[`${prefix}_max_age`] });
  if (config.modules.lending.enabled) {
    const m = config.modules.lending;
    modules.lending = { ...verified.modules.lending, collateral_oracle: verified.modules.oracle.address, collateral_oracle_code_hash: verified.modules.oracle.code_hash, collateral_oracle_pins: {}, ...metadata(m, 'collateral'), ...metadata(m, 'usdt') };
  }
  if (config.modules.stock_loans.enabled) { const m = config.modules.stock_loans; modules.stock_loans = { ...modules.stock_loans, ...verified.modules.stock_loans, ...metadata(m, 'usdt') }; }
  if (config.modules.buyback.enabled) { const m = config.modules.buyback; modules.buyback = { ...modules.buyback, ...verified.modules.buyback, router: m.router, router_code_hash: m.router_code_hash, router_pins: {}, official_token_code_hash: TOKEN_HASH, official_token_pins: { implementation: IMPLEMENTATION, implementation_code_hash: IMPLEMENTATION_HASH }, route: m.route }; }
  return { chain_id: 56, usdt_address: USDT, usdt_decimals: 18, modules };
}
async function run(mode, input) {
  guard(['plan', 'simulate', 'deploy', 'verify'].includes(mode), 'usage: bnb-finance-release.mjs plan|simulate|deploy|verify [public-config.json]');
  const manifest = read(files.manifest), config = validateConfig(read(input || files.input)), artifacts = loadArtifacts(config);
  const client = createPublicClient({ cacheTime: 0, chain: bsc, transport: http(process.env.TAB_BNB_RPC || 'https://bsc-dataseed.bnbchain.org', { timeout: 15_000, retryCount: 0 }) });
  const observed = await inspect(client, manifest, config);
  if (mode === 'plan' || mode === 'simulate') {
    guard(!fs.existsSync(files.journal) && !fs.existsSync(files.proof), 'existing deployment journal or receipts require reconciliation');
    if (mode === 'plan') guard(!fs.existsSync(files.plan), 'a reviewed plan already exists; do not regenerate its nonces');
    const snapshot = { nonce: await client.getTransactionCount({ address: SOURCE, blockTag: 'latest' }), pendingNonce: await client.getTransactionCount({ address: SOURCE, blockTag: 'pending' }), gasPrice: await client.getGasPrice(), balance: await client.getBalance({ address: SOURCE }), block: observed.block, blockHash: observed.blockHash };
    const plan = buildPlan(manifest, config, artifacts, snapshot), simulation = [];
    for (const tx of plan.transactions) {
      if (tx.address) { const code = await client.getCode({ address: tx.address }); guard(code == null || code === '0x', 'predicted deployment address already has code'); }
      if (tx.key === 'lending' || tx.key === 'direct_backing') {
        if (tx.key === 'direct_backing') { const a = await client.readContract({ address: PINS.backing.address, abi: BACKING_ABI, functionName: 'collateralAssets', args: [WBNB] }); guard(a[1] === 0, 'direct WBNB collateral is already configured; preserve its immutable terms'); }
        simulation.push({ key: tx.key, status: 'requires_prior_oracle_deployment', maximum_gas: tx.gas }); continue;
      }
      const request = { ...transaction(tx), account: SOURCE }; const estimated = await client.estimateGas(request); guard(estimated <= BigInt(tx.gas), 'constructor estimate exceeds planned gas'); await client.call(request);
      simulation.push({ key: tx.key, status: 'simulated', gas_estimate: estimated.toString() });
    }
    if (mode === 'plan') write(files.plan, plan);
    console.log(JSON.stringify({ status: mode === 'plan' ? 'planned' : 'simulated', plan: files.plan, plan_hash: digest(plan), maximum_cost_bnb: formatEther(BigInt(plan.maximum_cost_wei)), hard_cap_bnb: '0.005', transactions: plan.transactions.map(({ key, address, nonce }) => ({ key, address, nonce })), simulation }, null, 2)); return;
  }
  const plan = read(files.plan); validatePlan(plan, manifest, config, artifacts);
  const journal = fs.existsSync(files.journal) ? read(files.journal) : { id: ID, plan_hash: digest(plan), transactions: [] };
  guard(journal.id === ID && journal.plan_hash === digest(plan) && Array.isArray(journal.transactions) && journal.transactions.length <= plan.transactions.length, 'private journal does not belong to the reviewed plan');
  for (let i = 0; i < journal.transactions.length; i++) await validateSigned(journal.transactions[i], plan, plan.transactions[i]);
  if (mode === 'deploy') guard(process.env.TAB_FINANCE_DEPLOY_AUTHORIZATION === digest(plan), 'authorize the exact reviewed public plan hash before signing');
  if (!journal.transactions.length) guard(Date.now() >= Date.parse(plan.created_at) && Date.now() - Date.parse(plan.created_at) <= 3600_000, 'unsigned plan expired; preserve it for review');
  const confirmations = [];
  for (let i = 0; i < plan.transactions.length; i++) {
    const tx = plan.transactions[i]; let entry = journal.transactions[i];
    if (!entry) {
      guard(mode === 'deploy', 'not every planned deployment has a durable signed envelope');
      await inspect(client, manifest, config);
      const latest = await client.getTransactionCount({ address: SOURCE, blockTag: 'latest' }), pending = await client.getTransactionCount({ address: SOURCE, blockTag: 'pending' });
      guard(latest === tx.nonce && pending === tx.nonce, 'authority nonce changed; reconcile before signing');
      const spent = confirmations.reduce((s, x) => s + BigInt(x.gas_cost_wei), 0n), remaining = plan.transactions.slice(i).reduce((s, x) => s + BigInt(x.gas) * BigInt(x.gas_price_wei), 0n);
      guard(spent + remaining <= GAS_CAP && await client.getBalance({ address: SOURCE }) >= remaining, 'remaining aggregate gas or authority balance exceeds approved bounds');
      const request = { ...transaction(tx), account: SOURCE };
      guard(await client.estimateGas(request) <= BigInt(tx.gas), 'constructor estimate exceeds planned gas'); await client.call(request);
      const key = process.env.TAB_DEPLOYER_KEY; guard(key, 'inject the project deployer alias through Ryan Vault');
      const account = privateKeyToAccount(key.startsWith('0x') ? key : `0x${key}`); guard(eq(account.address, SOURCE), 'wrong project signing authority');
      const raw = await account.signTransaction(transaction(tx)); entry = { id: ID, plan_hash: digest(plan), key: tx.key, raw, hash: keccak256(raw), signed_at: new Date().toISOString() };
      await validateSigned(entry, plan, tx); journal.transactions.push(entry); write(files.journal, journal, 0o600);
    }
    const confirmation = await reconcile(client, entry, tx, mode === 'deploy'); confirmations.push(confirmation); entry.confirmation = confirmation; write(files.journal, journal, 0o600);
    if (tx.address) { const code = await client.getCode({ address: tx.address, blockNumber: BigInt(confirmation.block_number) }); guard(sourceMatches(code, artifacts[tx.key]), 'confirmed module runtime differs from planned source'); }
  }
  delete process.env.TAB_DEPLOYER_KEY;
  const verified = await verifyModules(client, manifest, config, artifacts, plan);
  await verifyCanonicalProof(client, verified, confirmations);
  const proof = { id: ID, status: 'deployed_verified', chain_id: 56, authority: SOURCE, protocol: PROTOCOL, official_tab_address: TOKEN, plan_hash: digest(plan), source_hash: plan.source_hash, transactions: confirmations, ...verified, gas_cost_bnb: formatEther(confirmations.reduce((s, x) => s + BigInt(x.gas_cost_wei), 0n)), funding_usdt_by_this_release: '0', stock_whitelist_changes: 0, direct_collateral_configuration: config.modules.lending.enabled && config.modules.lending.configure_direct_backing ? { asset: WBNB, oracle: verified.modules.oracle.address, ltv_bps: 5000, liquidation_bps: 7500, liquidation_bonus_bps: 500, debt_cap_units: DEBT_CAP.toString() } : null, verified_at: new Date().toISOString() };
  write(files.proof, proof); write(files.config, financeConfig(config, verified)); console.log(JSON.stringify({ ...proof, backend_config: files.config }, null, 2));
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[4] !== '--locked') {
    fs.mkdirSync(state, { recursive: true, mode: 0o700 }); fs.chmodSync(state, 0o700);
    const result = spawnSync('flock', ['-n', path.join(state, 'deployment.lock'), process.execPath, fileURLToPath(import.meta.url), process.argv[2] || '', process.argv[3] || files.input, '--locked'], { stdio: 'inherit' }); process.exitCode = result.status ?? 1;
  } else run(process.argv[2], process.argv[3]).catch(error => { delete process.env.TAB_DEPLOYER_KEY; console.error(error.message?.startsWith('Finance deployment:') || error.message?.startsWith('TAB activation:') || error.message?.startsWith('Finance pilot:') ? error.message : 'Finance deployment: RPC or signing failed. Preserve the durable private journal and reconcile its exact transaction hashes.'); process.exitCode = 1; });
}
