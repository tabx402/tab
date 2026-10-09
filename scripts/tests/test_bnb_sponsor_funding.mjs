import { test } from 'node:test';
import assert from 'node:assert/strict';
import { SOURCE, SPONSOR, validatePlan } from '../bnb-sponsor-fund.mjs';
const valid = () => ({ chain_id: 56, from: SOURCE, to: SPONSOR, amount_bnb: '0.01', value_wei: '10000000000000000', gas: '21000', gas_price_wei: '50000000', data: '0x', nonce: 5 });
test('accepts exact bounded BNB funding only', () => assert.equal(validatePlan(valid()), 10000000000000000n));
for (const patch of [
  { chain_id: 1 }, { to: SOURCE }, { from: SPONSOR }, { amount_bnb: '0' },
  { amount_bnb: '0.2', value_wei: '200000000000000000' }, { amount_bnb: '1e-2' },
  { value_wei: '10000000000000001' }, { gas: '21001' }, { data: '0x1234' },
  { gas_price_wei: '1000000001' }, { gas_price_wei: '0' }, { nonce: -1 }, { nonce: 1.5 },
]) test(`rejects changed funding authority or amount ${JSON.stringify(patch)}`, () => assert.throws(() => validatePlan({ ...valid(), ...patch })));
