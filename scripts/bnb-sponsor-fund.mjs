#!/usr/bin/env node
// One-time funding of Tab's BNB registration-only sponsor. Inject the deployer
// alias with Ryan Vault. A durable signed transaction makes retries idempotent.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createPublicClient, http, parseEther, formatEther, keccak256, parseTransaction, recoverTransactionAddress } from '../frontend/node_modules/viem/_esm/index.js';
import { privateKeyToAccount } from '../frontend/node_modules/viem/_esm/accounts/index.js';
import { bsc } from '../frontend/node_modules/viem/_esm/chains/index.js';

export const SOURCE = '0x00E128E7779EA927a087B40AdE268Dedc1B34A90';
export const SPONSOR = '0xA99Cf06fCdE993a6d2FaA73A2c82d67980Fd0416';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const planPath = path.join(root, 'contracts/deployments/bnb-sponsor-funding-56.json');
const journalPath = path.join(root, 'backend/data/bnb-sponsor-funding-private.json');
const equal = (a, b) => typeof a === 'string' && typeof b === 'string' && a.toLowerCase() === b.toLowerCase();
function guard(condition, message) { if (!condition) throw new Error(`Funding: ${message}`); }
function write(file, value, mode = 0o600) {
  const temp = file + '.tmp';
  fs.writeFileSync(temp, JSON.stringify(value, null, 2) + '\n', { mode, flag: 'wx' });
  fs.renameSync(temp, file);
}
export function validatePlan(plan) {
  guard(plan.chain_id === 56 && equal(plan.from, SOURCE) && equal(plan.to, SPONSOR), 'wrong chain or wallet');
  guard(typeof plan.amount_bnb === 'string' && /^(?:0|[1-9]\d*)(?:\.\d{1,18})?$/.test(plan.amount_bnb), 'invalid amount');
  const amount = parseEther(plan.amount_bnb);
  guard(amount > 0n && amount <= parseEther('0.1') && plan.value_wei === amount.toString(), 'amount exceeds the bounded funding range');
  guard(plan.gas === '21000' && plan.data === '0x', 'only a plain BNB transfer is allowed');
  guard(/^\d+$/.test(plan.gas_price_wei) && BigInt(plan.gas_price_wei) > 0n && BigInt(plan.gas_price_wei) <= 1_000_000_000n, 'gas price exceeds 1 gwei');
  guard(Number.isSafeInteger(plan.nonce) && plan.nonce >= 0, 'invalid nonce');
  return amount;
}
async function main() {
  const [mode, amountText] = process.argv.slice(2);
  guard(['plan', 'fund', 'verify'].includes(mode), 'usage: bnb-sponsor-fund.mjs plan AMOUNT_BNB | fund | verify');
  const client = createPublicClient({ chain: bsc, transport: http(process.env.TAB_BNB_RPC || 'https://bsc-dataseed.bnbchain.org', { retryCount: 0, timeout: 20_000 }) });
  guard(await client.getChainId() === 56, 'RPC chain mismatch');
  if (mode === 'plan') {
    guard(!fs.existsSync(planPath) && !fs.existsSync(journalPath), 'a funding record already exists; reconcile it instead');
    guard(amountText !== undefined, 'an explicit approved amount is required');
    const nonce = await client.getTransactionCount({ address: SOURCE, blockTag: 'pending' });
    guard(nonce === await client.getTransactionCount({ address: SOURCE, blockTag: 'latest' }), 'source wallet has pending transactions');
    const code = await client.getCode({ address: SPONSOR });
    guard(!code || code === '0x', 'sponsor is not a plain wallet');
    const gasPrice = await client.getGasPrice();
    const plan = { chain_id: 56, from: SOURCE, to: SPONSOR, amount_bnb: amountText, value_wei: parseEther(amountText).toString(), gas: '21000', gas_price_wei: gasPrice.toString(), data: '0x', nonce, created_at: new Date().toISOString(), automatic_topups: false };
    const amount = validatePlan(plan);
    guard(await client.getBalance({ address: SOURCE }) >= amount + 21_000n * gasPrice, 'insufficient funding wallet balance');
    write(planPath, plan, 0o644);
    console.log(JSON.stringify({ ...plan, maximum_gas_cost_bnb: formatEther(21_000n * gasPrice) }, null, 2));
    return;
  }
  const plan = JSON.parse(fs.readFileSync(planPath, 'utf8'));
  const value = validatePlan(plan);
  let journal = fs.existsSync(journalPath) ? JSON.parse(fs.readFileSync(journalPath, 'utf8')) : null;
  if (!journal) {
    guard(mode === 'fund', 'funding has not been signed');
    guard(Date.now() - Date.parse(plan.created_at) <= 30 * 60_000, 'funding plan expired');
    guard(plan.nonce === await client.getTransactionCount({ address: SOURCE, blockTag: 'pending' }), 'source nonce changed');
    guard(plan.nonce === await client.getTransactionCount({ address: SOURCE, blockTag: 'latest' }), 'source has a pending transaction');
    const key = process.env.TAB_DEPLOYER_KEY;
    guard(Boolean(key), 'inject the project deployer alias through Ryan Vault');
    delete process.env.TAB_DEPLOYER_KEY;
    const account = privateKeyToAccount(key.startsWith('0x') ? key : `0x${key}`);
    guard(equal(account.address, SOURCE), 'wrong signing wallet');
    const request = { chainId: 56, to: SPONSOR, value, data: '0x', gas: 21_000n, gasPrice: BigInt(plan.gas_price_wei), nonce: plan.nonce, type: 'legacy' };
    guard(await client.estimateGas({ ...request, account }) <= request.gas, 'transfer exceeds the gas limit');
    guard(await client.getBalance({ address: SOURCE }) >= value + request.gas * request.gasPrice, 'insufficient balance');
    const raw = await account.signTransaction(request);
    journal = { plan, hash: keccak256(raw), raw, signed_at: new Date().toISOString() };
    write(journalPath, journal);
  }
  guard(JSON.stringify(journal.plan) === JSON.stringify(plan) && keccak256(journal.raw) === journal.hash, 'funding journal differs from plan');
  const signed = parseTransaction(journal.raw);
  guard(signed.chainId === 56 && signed.type === 'legacy' && equal(signed.to, SPONSOR) && signed.value === value && (!signed.data || signed.data === '0x') && signed.nonce === plan.nonce && signed.gas === 21_000n && signed.gasPrice === BigInt(plan.gas_price_wei) && equal(await recoverTransactionAddress({ serializedTransaction: journal.raw }), SOURCE), 'signed transaction differs from the exact funding plan');
  if (mode === 'fund' && !journal.confirmed_at) {
    try {
      const hash = await client.sendRawTransaction({ serializedTransaction: journal.raw });
      guard(hash === journal.hash, 'broadcast hash mismatch');
    } catch {
      // The exact pre-recorded hash is authoritative after an ambiguous send.
      // Never sign a replacement or transfer a second time.
      console.log(JSON.stringify({ status: 'reconciling', transaction_hash: journal.hash }));
    }
  }
  const receipt = await client.waitForTransactionReceipt({ hash: journal.hash, confirmations: 3, timeout: 90_000 });
  const transaction = await client.getTransaction({ hash: journal.hash });
  const block = await client.getBlock({ blockNumber: receipt.blockNumber });
  guard(receipt.status === 'success' && receipt.blockHash === block.hash, 'transfer did not confirm canonically');
  guard(transaction.chainId === 56 && equal(transaction.from, SOURCE) && equal(transaction.to, SPONSOR) && transaction.value === value && transaction.input === '0x' && transaction.nonce === plan.nonce && transaction.gas === 21_000n && transaction.gasPrice === BigInt(plan.gas_price_wei), 'confirmed transfer differs from the plan');
  journal.confirmed_at = new Date().toISOString();
  write(journalPath, journal);
  const proof = { ...plan, transaction_hash: journal.hash, block_number: receipt.blockNumber.toString(), gas_cost_bnb: formatEther(receipt.gasUsed * receipt.effectiveGasPrice), confirmed_at: journal.confirmed_at, sponsor_balance_bnb: formatEther(await client.getBalance({ address: SPONSOR })) };
  // Preserve the immutable plan separately from the confirmation evidence.
  write(planPath.replace('.json', '-receipt.json'), proof, 0o644);
  console.log(JSON.stringify(proof, null, 2));
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => {
    console.error(error.message?.startsWith('Funding:') ? error.message : 'Funding: RPC or signing operation failed; reconcile the recorded transaction before retrying.');
    process.exitCode = 1;
  });
}
