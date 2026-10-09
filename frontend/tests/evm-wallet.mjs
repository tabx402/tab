import { chromium } from '@playwright/test';
import assert from 'node:assert/strict';

const origin = process.env.TAB_TEST_ORIGIN || 'http://127.0.0.1:5198';
const browser = await chromium.launch();
const page = await browser.newPage();
try {
  await page.goto(`${origin}/tests/wizard.html`, { waitUntil: 'domcontentloaded' });
  const result = await page.evaluate(async () => {
    const evm = await import('/src/lib/evm.ts');
    const amounts = await import('/src/lib/amounts.ts');
    const {walletPromptRejected} = await import('/src/components/AgentWalletActions.tsx');
    const owner = '0x1111111111111111111111111111111111111111';
    const destination = '0x2222222222222222222222222222222222222222';
    const hash = `0x${'a'.repeat(64)}`;
    const calls = [];
    let connectedChain = '0x1';
    let connectedOwner = owner;
    let switchWorks = true;
    let failSend = false;
    const wallet = {
      address: owner,
      switchChain: async chain => { calls.push(['switch', chain]); if (switchWorks) connectedChain = `0x${chain.toString(16)}`; },
      getEthereumProvider: async () => ({ request: async ({ method, params }) => {
        calls.push([method, params]);
        if (method === 'eth_chainId') return connectedChain;
        if (method === 'eth_accounts') return [connectedOwner];
        if (method === 'eth_sendTransaction') { if(failSend)throw Error('RPC response lost.');return hash; }
        if (method === 'personal_sign') return `0x${'a'.repeat(130)}`;
        throw Error(`Unexpected mock RPC ${method}`);
      }}),
    };
    const tx = { to: destination, data: '0x12345678', value: '0x1', chainId: '0x38' };
    const sent = await evm.sendBnbTransaction(wallet, tx, owner.toUpperCase().replace('0X', '0x'));
    const client = await evm.bnbWalletClient(wallet, owner);
    await client.signMessage({ message: 'tabagents.io BNB Smart Chain 56 registration' });
    const rejected = [],preflightTags=[];
    for (const candidate of [ { ...tx, chainId: '0x1' }, { ...tx, to: 'not-an-address' }, { ...tx, data: '0x1' }, { ...tx, value: '-1' } ]) {
      try { await evm.sendBnbTransaction(wallet, candidate, owner); } catch (error) { rejected.push(error.message);preflightTags.push(error instanceof evm.NoWalletSubmissionError && walletPromptRejected(error)); }
    }
    let wrongOwner = false, wrongNetwork = false, changedAccount = false;
    try { await evm.sendBnbTransaction(wallet, tx, destination); } catch(error) { wrongOwner = true;preflightTags.push(error instanceof evm.NoWalletSubmissionError && walletPromptRejected(error)); }
    connectedChain = '0x1'; switchWorks = false;
    try { await evm.sendBnbTransaction(wallet, tx, owner); } catch(error) { wrongNetwork = true;preflightTags.push(error instanceof evm.NoWalletSubmissionError && walletPromptRejected(error)); }
    switchWorks = true; connectedOwner = destination;
    try { await evm.sendBnbTransaction(wallet, tx, owner); } catch(error) { changedAccount = true;preflightTags.push(error instanceof evm.NoWalletSubmissionError && walletPromptRejected(error)); }
    const preflightSendCount=calls.filter(([method])=>method==='eth_sendTransaction').length;
    connectedOwner=owner;failSend=true;let unknownSendTagged=null,unknownSendRetryable=null;
    try{await evm.sendBnbTransaction(wallet,tx,owner);}catch(error){unknownSendTagged=error instanceof evm.NoWalletSubmissionError;unknownSendRetryable=walletPromptRejected(error);}
    const precise = '25.123456789123456789';
    return {
      sent, calls, rejected, wrongOwner, wrongNetwork, changedAccount,preflightTags,preflightSendCount,unknownSendTagged,unknownSendRetryable,
      units: amounts.usdtUnits(precise).toString(),
      roundTrip: amounts.amountFromUnits(amounts.usdtUnits(precise)),
      dust: amounts.formatAmount('0.000000000000000001', 2),
      tooPrecise: amounts.usdtUnits('1.0000000000000000001'),
      scientific: amounts.usdtUnits('1e-18'),
      explorer: evm.explorer(hash),
      wrongChainExplorer: evm.explorer(hash, 'tx', 1),
      exactRecovery: evm.matchesTransaction({chainId:'0x38',from:owner,to:destination,input:tx.data,value:tx.value},tx,owner),
      wrongRecovery: evm.matchesTransaction({chainId:'0x38',from:owner,to:destination,input:'0xdeadbeef',value:tx.value},tx,owner),
    };
  });
  assert.equal(result.exactRecovery,true);assert.equal(result.wrongRecovery,false);
  assert.equal(result.sent, `0x${'a'.repeat(64)}`);
  assert.deepEqual(result.calls[0], ['switch', 56]);
  const submitted = result.calls.filter(([method]) => method === 'eth_sendTransaction');
  assert.equal(result.preflightSendCount,1,'invalid requests and wrong wallets or networks must never reach wallet submission');
  assert.equal(result.preflightTags.length,7);assert.ok(result.preflightTags.every(Boolean),'all proven pre-send errors remain retryable');
  assert.ok(submitted.length>result.preflightSendCount,'unknown send error occurs after calling the provider');assert.equal(result.unknownSendTagged,false);assert.equal(result.unknownSendRetryable,false,'unknown provider errors must remain uncertain');
  assert.equal(submitted[0][1][0].value, '0x1', 'native BNB value remains exact wei');
  assert.equal(submitted[0][1][0].to, '0x2222222222222222222222222222222222222222');
  assert.equal(result.calls.filter(([method]) => method === 'personal_sign').length, 1);
  assert.equal(result.rejected.length, 4);
  assert.equal(result.wrongOwner && result.wrongNetwork && result.changedAccount, true);
  assert.equal(result.units, '25123456789123456789');
  assert.equal(result.roundTrip, '25.123456789123456789');
  assert.equal(result.dust, '0.000000000000000001');
  assert.equal(result.tooPrecise, null);
  assert.equal(result.scientific, null);
  assert.equal(result.explorer, `https://bscscan.com/tx/0x${'a'.repeat(64)}`);
  assert.equal(result.wrongChainExplorer, undefined);
  console.log('BNB wallet network switching, account binding, transaction validation, personal signing and exact 18-decimal USDT amounts passed without sending a transaction.');
} finally {
  await browser.close();
}
