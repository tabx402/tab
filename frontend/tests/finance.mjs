import { chromium, expect } from '@playwright/test';
import { encodeFunctionData, erc20Abi, parseAbi } from 'viem';
import assert from 'node:assert/strict';
const origin = process.env.TAB_TEST_ORIGIN || 'http://127.0.0.1:5222';
const browser = await chromium.launch();
const page = await browser.newPage({ reducedMotion: 'reduce', viewport: { width: 1440, height: 1000 } });
const errors = []; page.on('pageerror', reason => errors.push(reason.message));
const owner = '0x1111111111111111111111111111111111111111', usdt = '0x55d398326f99059fF775485246999027B3197955';
const pool = '0x2222222222222222222222222222222222222222', stocks = '0x3333333333333333333333333333333333333333', buyback = '0x4444444444444444444444444444444444444444', collateral = '0x5555555555555555555555555555555555555555';
const lenderAbi = parseAbi(['function deposit(uint256 assets,address receiver)', 'function redeem(uint256 shares,address receiver,address owner)', 'function approveLoan((bytes32 id,bytes32 agent,bytes32 job,address signer,uint256 principal,uint256 perCall,uint256 dailyCap,uint64 expiresAt,uint64 tools,address[] recipients) t)', 'function acceptLoan(bytes32 id)', 'function spendLoan(bytes32 id,address recipient,uint256 amount,uint64 tool,bytes32 request,bytes32 receipt)', 'function repayLoan(bytes32 id,uint256 amount)', 'function closeLoan(bytes32 id)', 'function pledgeCollateral(bytes32 id) payable', 'function withdrawCollateral(bytes32 id,uint256 amount)', 'function liquidateLoan(bytes32 id,uint256 maxRepay,uint256 minimumCollateral)']);
const stockAbi = parseAbi(['function deposit(uint256 assets,address receiver)', 'function redeem(uint256 shares,address receiver,address owner)', 'function borrow(bytes32 id,address token,uint256 collateralAmount,uint256 principal)', 'function addCollateral(bytes32 id,uint256 amount)', 'function withdrawCollateral(bytes32 id,uint256 amount)', 'function repay(bytes32 id,uint256 amount)', 'function liquidate(bytes32 id,uint256 principal,uint256 minimumCollateral)']);
const buybackAbi = parseAbi(['function fund(uint256 amount)', 'function execute(uint256 amount,uint256 minimumTokens,uint256 deadline)']);
const jobId = '99999999999999999999999999999999', borrowerAgentId = '77777777777777777777777777777777', borrower = '0x7777777777777777777777777777777777777777', merchant = '0x6666666666666666666666666666666666666666';
const loanId = '0x' + 'b'.repeat(64), registryId = borrower + borrowerAgentId.slice(8);
const expires = Math.floor(Date.now() / 1000) + 3600;
const advanceRequest = { id: 'qa-request', loan_id: loanId, agent_id: borrowerAgentId, agent_name: 'borrower QA', job_id: jobId, job_title: 'funded QA work', borrower, amount: '1', per_call: '0.1', daily_cap: '1', expires_at: expires, signer: borrower, tools: ['x402'], recipients: [merchant], status: 'requested', created_at: new Date().toISOString(), public: false };
const system = { chain_id: 56, currency: 'USDT', source: 'verified_onchain', modules: {
  lending: { status: 'verified', address: pool, reason: null, available_usdt: '20', total_assets_usdt: '25', outstanding_usdt: '5', total_shares: '25', paused: false, underwriter: owner },
  stock_loans: { status: 'verified', address: stocks, reason: null, available_usdt: '20', total_assets_usdt: '25', outstanding_usdt: '5', total_shares: '25', paused: false, underwriter: owner, assets: [{ token: collateral, symbol: 'QA-STOCK', decimals: 6, feed: merchant, ltv_bps: 5000, liquidation_bps: 6500, max_age: 3600, market_open: true, status: 'verified', price_usdt: '10' }] },
  buyback: { status: 'not_deployed', address: null, reason: 'Optional module is not deployed.', official_token: null, spent_usdt: null, tokens_burned: null, available_usdt: null, paused: false, operator: null },
} };
let issued = null;
let prepared = [];
let submitted = 0;
let oracleFails = true;
let pendingJobs = [];
let advanceLoans = [];
const account = () => ({ system, wallet: owner, status: 'ready', lending: { shares: '5', max_redeem: '2', share_value_usdt: '5', loans: advanceLoans }, stock_loans: { shares: '0', max_redeem: '0', share_value_usdt: '0', loans: [] }, requests: [advanceRequest], pending_intents: issued ? [issued] : [], pending_job_intents: pendingJobs, roles: { underwriter: true, buyback_operator: false } });
function buildIntent(body) {
  let details = { ...body, finance_module: 'lending', registry_id: '0x' + '1'.repeat(64) };
  let to = pool, data;
  if (body.action === 'pool_deposit') data = encodeFunctionData({ abi: lenderAbi, functionName: 'deposit', args: [BigInt(body.amount) * 10n ** 18n, owner] });
  else if (body.action === 'advance_approve') {
    details = { ...body, registry_id: registryId, job_onchain_id: '0x' + jobId.padStart(64, '0'), finance_module: 'lending' };
    data = encodeFunctionData({ abi: lenderAbi, functionName: 'approveLoan', args: [{ id: loanId, agent: registryId, job: details.job_onchain_id, signer: borrower, principal: 10n ** 18n, perCall: 10n ** 17n, dailyCap: 10n ** 18n, expiresAt: BigInt(expires), tools: 16n, recipients: [merchant] }] });
  } else if (body.action === 'advance_pledge') data=encodeFunctionData({abi:lenderAbi,functionName:'pledgeCollateral',args:[body.loan_id]});
  else throw Error(`Unexpected fixture action ${body.action}`);
  const tx = { to, data, value: body.action === 'advance_pledge' ? String(BigInt(body.collateral_amount.replace('.','')) * 10n ** BigInt(18 - (body.collateral_amount.split('.')[1]?.length || 0))) : '0', chainId: '56' };
  const transactions = body.action === 'pool_deposit' ? [{ to: usdt, data: encodeFunctionData({ abi: erc20Abi, functionName: 'approve', args: [pool, BigInt(body.amount) * 10n ** 18n] }), value: '0', chainId: '56' }, tx] : [tx];
  return { id: 'local-finance-intent', agent_id: '1'.repeat(32), action: `finance_${body.action}`, chain_id: 56, network: 'mainnet', sender: owner, to, data, value: tx.value, transaction: tx, transactions, expires_at: new Date(Date.now() + 300000).toISOString(), details };
}
await page.route('**/api/**', async route => {
  const path = new URL(route.request().url()).pathname;
  if (path.endsWith('/finance')) return route.fulfill({ json: account() });
  if (path.endsWith('/finance/prepare')) { const body = route.request().postDataJSON(); prepared.push(body); issued = buildIntent(body); return route.fulfill({ json: issued }); }
  if (path.endsWith('/submitted')) { submitted++; return route.fulfill({ json: { status: 'submitted' } }); }
  if (path.endsWith('/confirm')) { issued = null; return route.fulfill({ json: { status: 'confirmed' } }); }
  if (path === '/api/account/jobs') return route.fulfill({ json: [] });
  if (path === '/api/x402/system') return route.fulfill({ json: { merchants: [], settlement_enabled: false } });
  if (path === '/api/finance/quote') { if (oracleFails) return route.fulfill({ status: 503, json: { detail: 'Collateral oracle is stale.' } }); return route.fulfill({ json: { status: 'verified', action: 'stock_borrow', chain_id: 56, currency: 'USDT', token: collateral, decimals: 6, maximum_borrow_usdt: '5', collateral_value_usdt: '10' } }); }
  return route.fulfill({ json: [] });
});
try {
  await page.goto(`${origin}/tests/finance.html`, { waitUntil: 'networkidle' });
  await expect(page.getByText('shares you can redeem', { exact: true })).toHaveCount(2);
  await page.getByLabel('finance amount', { exact: true }).fill('1');
  await page.getByRole('button', { name: 'review finance transaction', exact: true }).click();
  await expect(page.getByRole('button', { name: 'confirm finance in wallet', exact: true })).toBeVisible();
  await expect(page.locator('.finance-review')).toContainText(pool);
  const goodIntent = structuredClone(issued);
  const valid = await page.evaluate(({ intent, data, input }) => window.checkFinancePayload(intent, data, input), { intent: goodIntent, data: account(), input: prepared[0] });
  assert.equal(valid, 'accepted');
  const tampered = structuredClone(goodIntent);
  const badData = encodeFunctionData({ abi: lenderAbi, functionName: 'deposit', args: [2n * 10n ** 18n, owner] });
  tampered.data = badData; tampered.transaction.data = badData; tampered.transactions.at(-1).data = badData;
  assert.match(await page.evaluate(({ intent, data, input }) => window.checkFinancePayload(intent, data, input), { intent: tampered, data: account(), input: prepared[0] }), /calldata differs/);
  const unlimited = structuredClone(goodIntent); unlimited.transactions[0].data = encodeFunctionData({ abi: erc20Abi, functionName: 'approve', args: [pool, 2n ** 256n - 1n] });
  assert.match(await page.evaluate(({ intent, data, input }) => window.checkFinancePayload(intent, data, input), { intent: unlimited, data: account(), input: prepared[0] }), /exact reviewed token amount/);
  await page.getByRole('button', { name: 'confirm finance in wallet', exact: true }).click();
  await expect(page.getByText('Wallet response lost after submission.', { exact: true })).toBeVisible();
  assert.equal(await page.locator('body').getAttribute('data-sends'), '1');
  await expect(page.getByRole('button', { name: 'confirm finance in wallet', exact: true })).toHaveCount(0);
  await page.reload({ waitUntil: 'networkidle' });
  await expect(page.getByText('The wallet interaction may have submitted this action.', { exact: false })).toBeVisible();
  await expect(page.getByRole('button', { name: 'confirm finance in wallet', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'review finance transaction', exact: true })).toBeDisabled();
  await page.getByLabel('finance transaction hash', { exact: true }).fill('0x' + 'a'.repeat(64));
  await page.getByRole('button', { name: 'verify finance transaction', exact: true }).click();
  await expect(page.getByText('Finance transaction confirmed on BNB mainnet.', { exact: true })).toBeVisible();
  assert.equal(submitted, 1);
  assert.equal(await page.evaluate(() => localStorage.getItem('tab:finance:pending:' + '1'.repeat(32))), null);
  await page.getByRole('button', { name: 'job advances', exact: true }).click();
  await page.getByLabel('advance request', { exact: true }).selectOption('qa-request');
  await page.getByRole('button', { name: 'review finance transaction', exact: true }).click();
  await expect(page.getByRole('button', { name: 'confirm finance in wallet', exact: true })).toBeVisible();
  assert.equal(prepared.at(-1).amount, '1', 'Approval must preserve the requested principal.');
  const advanceIntent = structuredClone(issued);
  assert.equal(await page.evaluate(({ intent, data, input }) => window.checkFinancePayload(intent, data, input), { intent: advanceIntent, data: account(), input: prepared.at(-1) }), 'accepted');
  const wrongJob = structuredClone(advanceIntent); wrongJob.details.job_onchain_id = '0x' + 'd'.repeat(64);
  assert.match(await page.evaluate(({ intent, data, input }) => window.checkFinancePayload(intent, data, input), { intent: wrongJob, data: account(), input: prepared.at(-1) }), /different borrower agent or job/);
  const validationData = account();
  validationData.system = structuredClone(system);
  validationData.system.modules.buyback = { status: 'verified', address: buyback, reason: null, official_token: collateral, token_decimals: 6, spent_usdt: '0', tokens_burned: '0', available_usdt: '10', operator: owner };
  validationData.roles.buyback_operator = true;
  validationData.stock_loans.loans = [{ id: loanId, borrower: owner, token: collateral, symbol: 'QA-STOCK', decimals: 6, collateral: '2', debt: '1', loss: '0', collateral_value_usdt: '20', maximum_borrow_usdt: '10', liquidation_debt_usdt: '13', liquidatable: true, oracle_status: 'verified', actions: { add_collateral: true, withdraw: true, repay: true, liquidate: true } }];
  const principal = 10n ** 18n;
  const commitment = '0x' + 'c'.repeat(64);
  const swapDeadline = Math.floor(Date.now() / 1000) + 300;
  const cases = [
    ['pool_deposit', lenderAbi, 'deposit', [principal, owner], pool, { amount: '1' }, usdt, principal],
    ['pool_redeem', lenderAbi, 'redeem', [principal, owner, owner], pool, { amount: '1' }],
    ['stock_deposit', stockAbi, 'deposit', [principal, owner], stocks, { amount: '1' }, usdt, principal],
    ['stock_redeem', stockAbi, 'redeem', [principal, owner, owner], stocks, { amount: '1' }],
    ['advance_accept', lenderAbi, 'acceptLoan', [loanId], pool, { amount: '0', loan_id: loanId }],
    ['advance_spend', lenderAbi, 'spendLoan', [loanId, merchant, principal, 16n, commitment, commitment], pool, { amount: '1', loan_id: loanId, recipient: merchant, tool: 'x402', request_hash: commitment, receipt_hash: commitment }],
    ['advance_repay', lenderAbi, 'repayLoan', [loanId, principal], pool, { amount: '1', loan_id: loanId }, usdt, principal],
    ['advance_close', lenderAbi, 'closeLoan', [loanId], pool, { amount: '0', loan_id: loanId }],
    ['advance_pledge', lenderAbi, 'pledgeCollateral', [loanId], pool, { amount: '0', loan_id: loanId, collateral_amount: '0.1' }],
    ['advance_withdraw_collateral', lenderAbi, 'withdrawCollateral', [loanId, 10n ** 17n], pool, { amount: '0', loan_id: loanId, collateral_amount: '0.1' }],
    ['advance_liquidate', lenderAbi, 'liquidateLoan', [loanId, principal, 10n ** 15n], pool, { amount: '1', loan_id: loanId, minimum_out: '0.001', liquidation_repay_usdt: '0.75' }, usdt, 75n * 10n ** 16n],
    ['stock_borrow', stockAbi, 'borrow', [loanId, collateral, 2_000_000n, principal], stocks, { amount: '1', loan_id: loanId, token_address: collateral, collateral_amount: '2' }, collateral, 2_000_000n],
    ['stock_add_collateral', stockAbi, 'addCollateral', [loanId, 500_000n], stocks, { amount: '0', loan_id: loanId, token_address: collateral, collateral_amount: '0.5' }, collateral, 500_000n],
    ['stock_withdraw', stockAbi, 'withdrawCollateral', [loanId, 500_000n], stocks, { amount: '0', loan_id: loanId, token_address: collateral, collateral_amount: '0.5' }],
    ['stock_repay', stockAbi, 'repay', [loanId, principal], stocks, { amount: '1', loan_id: loanId, token_address: collateral }, usdt, principal],
    ['stock_liquidate', stockAbi, 'liquidate', [loanId, principal, 100_000n], stocks, { amount: '1', loan_id: loanId, token_address: collateral, minimum_out: '0.1' }, usdt, principal],
    ['buyback_fund', buybackAbi, 'fund', [principal], buyback, { amount: '1' }, usdt, principal],
    ['buyback_execute', buybackAbi, 'execute', [principal, 1_000_000n, BigInt(swapDeadline)], buyback, { amount: '1', minimum_out: '1', deadline: swapDeadline }],
  ];
  for (const [action, abi, functionName, args, to, fields, approvalToken, approvalAmount] of cases) {
    const body = { action, ...fields };
    const encoded = encodeFunctionData({ abi, functionName, args });
    const tx = { to, data: encoded, value: action === 'advance_pledge' ? String(10n ** 17n) : '0', chainId: '56' };
    const transactions = approvalToken ? [{ to: approvalToken, data: encodeFunctionData({ abi: erc20Abi, functionName: 'approve', args: [to, approvalAmount] }), value: '0', chainId: '56' }, tx] : [tx];
    const intent = { ...goodIntent, action: `finance_${action}`, to, data: encoded, value: tx.value, transaction: tx, transactions, details: { ...body, token_decimals: 6 } };
    assert.equal(await page.evaluate(({ intent, data, input }) => window.checkFinancePayload(intent, data, input), { intent, data: validationData, input: body }), 'accepted', `${action} exact wallet payload rejected`);
    if (action === 'advance_pledge') {
      const wrongNative = structuredClone(intent); wrongNative.value = wrongNative.transaction.value = wrongNative.transactions.at(-1).value = String(2n * 10n ** 17n);
      assert.match(await page.evaluate(({ intent, data, input }) => window.checkFinancePayload(intent, data, input), { intent: wrongNative, data: validationData, input: body }), /differs from its reviewed summary/, 'Native BNB pledge must bind the exact amount.');
      const extraApproval = structuredClone(intent); extraApproval.transactions.unshift({ to: usdt, data: encodeFunctionData({abi: erc20Abi,functionName:'approve',args:[pool,principal]}),value:'0',chainId:'56'});
      assert.match(await page.evaluate(({ intent, data, input }) => window.checkFinancePayload(intent, data, input), { intent: extraApproval, data: validationData, input: body }), /does not need a token approval/, 'Native pledge never approves a token.');
    }
    if (action === 'advance_liquidate') {
      const excessApproval=structuredClone(intent); excessApproval.transactions[0].data=encodeFunctionData({abi:erc20Abi,functionName:'approve',args:[pool,principal]});
      assert.match(await page.evaluate(({ intent, data, input }) => window.checkFinancePayload(intent, data, input), { intent: excessApproval, data: validationData, input: body }), /exact reviewed token amount/, 'Liquidation approves actual repayment only.');
    }
    if (['stock_add_collateral', 'stock_withdraw'].includes(action)) {
      const changedAssetData = structuredClone(validationData);
      changedAssetData.system.modules.stock_loans.assets.find(asset => asset.token === collateral).decimals = 18;
      assert.equal(await page.evaluate(({ intent, data, input }) => window.checkFinancePayload(intent, data, input), { intent, data: changedAssetData, input: body }), 'accepted', `${action} must retain the existing loan's collateral precision`);
    }
    const differentRecipient = structuredClone(intent); differentRecipient.transactions.at(-1).to = merchant;
    assert.match(await page.evaluate(({ intent, data, input }) => window.checkFinancePayload(intent, data, input), { intent: differentRecipient, data: validationData, input: body }), /differs from its reviewed summary/, `${action} destination must stay bound`);
  }
  issued = { ...goodIntent, agent_id: 'f'.repeat(32) };
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: 'networkidle' });
  await expect(page.getByRole('button', { name: 'confirm finance in wallet', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'review finance transaction', exact: true })).toBeDisabled();
  await expect(page.getByRole('link', { name: 'open the agent with the pending action', exact: true })).toHaveAttribute('href', `/account?agent=${'f'.repeat(32)}&section=finance`);
  issued = null;
  pendingJobs = [{ confirmed: 0, tx_hash: '0x' + 'a'.repeat(64), intent: { sender: owner } }];
  await page.reload({ waitUntil: 'networkidle' });
  await expect(page.getByRole('button', { name: 'review finance transaction', exact: true })).toBeDisabled();
  await expect(page.getByRole('link', { name: 'finish the pending job transaction', exact: true })).toBeVisible();
  pendingJobs = [];
  issued = null;
  await page.evaluate(() => localStorage.clear());
  advanceLoans=[{id:loanId,agent:'0x'+'1'.repeat(64),job:jobId,borrower:owner,signer:owner,principal:'1',available:'1',debt:'0',loss:'0',spent:'0',repaid:'0',per_call:'0.1',daily_cap:'1',expires_at:expires,tools:['x402'],recipients:[merchant],accepted:true,closed:false,secured:true,collateral_symbol:'BNB',collateral:'0',collateral_value_usdt:'0',maximum_borrow_usdt:'0',available_borrowing_usdt:'0',liquidation_debt_usdt:'0',liquidatable:false,oracle_status:'verified',actions:{accept:false,spend:false,repay:false,close:true,pledge:true,withdraw_collateral:false,liquidate:false}}];
  await page.goto(`${origin}/tests/finance.html?module=advances`, {waitUntil:'networkidle'});
  await expect(page.getByText('pledged collateral',{exact:true})).toBeVisible();
  await page.getByLabel('finance action',{exact:true}).selectOption('advance_pledge');
  await page.getByLabel('job advance',{exact:true}).selectOption(loanId);
  await page.getByLabel('advance BNB collateral amount',{exact:true}).fill('0.123456789012345678');
  await page.getByRole('button',{name:'review finance transaction',exact:true}).click();
  await expect(page.getByRole('button',{name:'confirm finance in wallet',exact:true})).toBeVisible();
  assert.equal(issued.transactions.length,1);
  assert.equal(BigInt(issued.value),123456789012345678n,'Native collateral keeps all 18 decimal places.');
  issued=null; await page.evaluate(()=>localStorage.clear());
  advanceLoans[0]={...advanceLoans[0],debt:'1',collateral:'0.001',collateral_value_usdt:null,maximum_borrow_usdt:null,available_borrowing_usdt:null,liquidation_debt_usdt:null,oracle_status:'unavailable',actions:{...advanceLoans[0].actions,repay:true}};
  await page.reload({waitUntil:'networkidle'});
  await expect(page.getByText('collateral price unavailable',{exact:true})).toBeVisible();
  await page.getByLabel('finance action',{exact:true}).selectOption('advance_repay');
  await expect(page.getByLabel('job advance',{exact:true}).locator(`option[value="${loanId}"]`)).toHaveCount(1);
  await page.getByLabel('job advance',{exact:true}).selectOption(loanId);
  await expect(page.getByRole('button',{name:'review finance transaction',exact:true})).toBeEnabled();
  advanceLoans=[];
  await page.goto(`${origin}/tests/finance.html?module=stocks`, { waitUntil: 'networkidle' });
  await expect(page.getByLabel('finance action', { exact: true })).toHaveValue('stock_borrow');
  await page.getByLabel('approved stock token', { exact: true }).selectOption(collateral);
  await page.getByLabel('collateral amount', { exact: true }).fill('1.0000001');
  await page.getByRole('button', { name: 'get borrowing quote', exact: true }).click();
  await expect(page.getByText("Enter an exact amount within the token's decimal precision.", { exact: true })).toBeVisible();
  await page.getByLabel('collateral amount', { exact: true }).fill('1');
  await page.getByRole('button', { name: 'get borrowing quote', exact: true }).click();
  await expect(page.getByText('Collateral oracle is stale.', { exact: true })).toBeVisible();
  const before = prepared.length;
  await page.getByRole('button', { name: 'review finance transaction', exact: true }).click();
  await expect(page.getByText('Request a fresh quote for the exact amounts before preparing this action.', { exact: true })).toBeVisible();
  assert.equal(prepared.length, before, 'Stale or missing quotes must not prepare a wallet action.');
  await page.goto(`${origin}/tests/finance.html?module=buyback`, { waitUntil: 'networkidle' });
  await expect(page.getByRole('button', { name: 'TAB buybacks', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('button', { name: 'review finance transaction', exact: true })).toBeDisabled();
  await expect(page.getByText('awaiting verification', { exact: true })).toHaveCount(3);
  for (const width of [320, 390, 768]) { await page.setViewportSize({ width, height: 844 }); assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, `${width}px finance overflow`); }
  assert.deepEqual(errors, []);
  console.log('All 19 finance wallet actions, exact native BNB pledge, actual liquidation repayment, approvals/calldata/borrower-job binding, canonical advance principal, uncertain submission recovery, cross-agent/job blocking, quote precision/stale-oracle gates, deep links and mobile layouts passed.');
} finally { await browser.close(); }
