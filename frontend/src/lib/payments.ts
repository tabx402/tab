import { decodeFunctionData, erc20Abi, isAddress, type Hex } from "viem";
import type { ConnectedWallet } from "@privy-io/react-auth";
import { BNB_CHAIN_ID, USDT_ADDRESS, bnbWalletClient, sameAddress, validateTransaction, type EvmTransaction } from "./evm";
import { usdtUnits } from "./amounts";

export const PERMIT2 = "0x000000000022D473030F116dDEE9F6B43aC78BA3";
export const EXACT_PROXY = "0x402085c248EeA27D92E8b30b2C58ed07f9E20001";
export type Merchant = { id: string; recipient: string; resource_url: string; capability_status: string; settlement_enabled: boolean };
export type PaymentSystem = { status: string; network: string; currency: string; asset: string; merchants: Merchant[]; settlement_enabled: boolean };
export type PaymentOffer = { amount_units: string; resource: { url: string; description?: string }; accepted: { amount: string; asset: string; network: string; payTo: string; extra: { assetTransferMethod?: string } } };
export type PermitData = { domain: { name: string; chainId: number; verifyingContract: string }; primaryType: string; types: Record<string, {name:string;type:string}[]>; message: { permitted: {token:string;amount:string}; spender:string; nonce:string; deadline:string; witness:{to:string;validAfter:string} } };
export type PaymentQuote = { quote_id?: string; provider:string; sender:string; chain_id:number; currency:string; status:string; offer:PaymentOffer; transactions?:EvmTransaction[]; typed_data?:PermitData; expires_at?:number };
export type PaymentHistory = { quote_id:string; provider:string; status:string; expires_at:number; amount:string; tx_hash?:string|null };
const permitTypes = {
  EIP712Domain: [{name:"name",type:"string"},{name:"chainId",type:"uint256"},{name:"verifyingContract",type:"address"}],
  PermitWitnessTransferFrom: [{name:"permitted",type:"TokenPermissions"},{name:"spender",type:"address"},{name:"nonce",type:"uint256"},{name:"deadline",type:"uint256"},{name:"witness",type:"Witness"}],
  TokenPermissions: [{name:"token",type:"address"},{name:"amount",type:"uint256"}],
  Witness: [{name:"to",type:"address"},{name:"validAfter",type:"uint256"}],
};
const uint = (value: string) => /^(0|[1-9]\d*)$/.test(value) && BigInt(value) < 2n ** 256n;
export function validatePaymentQuote(quote: PaymentQuote, merchant: Merchant, owner: string, perCall: string, dailyCap: string) {
  const offer = quote.offer, maxCall = usdtUnits(perCall), maxDay = usdtUnits(dailyCap);
  if (quote.chain_id !== BNB_CHAIN_ID || quote.currency !== "USDT" || !sameAddress(quote.sender, owner) || quote.provider !== merchant.id || !uint(offer.amount_units) || BigInt(offer.amount_units) <= 0n || maxCall === null || maxDay === null || BigInt(offer.amount_units) > maxCall || BigInt(offer.amount_units) > maxDay || offer.accepted.amount !== offer.amount_units || offer.accepted.network !== "eip155:56" || !sameAddress(offer.accepted.asset, USDT_ADDRESS) || !sameAddress(offer.accepted.payTo, merchant.recipient) || offer.resource.url !== merchant.resource_url || offer.accepted.extra.assetTransferMethod !== "permit2") throw Error("The quote differs from this agent's approved USDT payment.");
  if (quote.status === "approval_required") {
    const transactions = quote.transactions;
    if (!transactions?.length || transactions.length > 2) throw Error("The USDT approval is invalid.");
    transactions.forEach((transaction,index) => {
      const tx = validateTransaction(transaction);
      const decoded = decodeFunctionData({ abi: erc20Abi, data: tx.data });
      const expected = index === transactions.length - 1 ? BigInt(offer.amount_units) : 0n;
      if (!sameAddress(tx.to, USDT_ADDRESS) || tx.value !== 0n || decoded.functionName !== "approve" || !sameAddress(String(decoded.args[0]), PERMIT2) || decoded.args[1] !== expected) throw Error("Approve only the quoted USDT amount for Permit2.");
    });
    return;
  }
  const data = quote.typed_data, now = Math.floor(Date.now()/1000);
  if (quote.status !== "requires_wallet_authorization" || !quote.quote_id || !data || data.domain.name !== "Permit2" || data.domain.chainId !== BNB_CHAIN_ID || !sameAddress(data.domain.verifyingContract, PERMIT2) || data.primaryType !== "PermitWitnessTransferFrom" || Object.keys(data.types).length !== Object.keys(permitTypes).length || Object.entries(permitTypes).some(([name, fields]) => JSON.stringify(data.types[name]) !== JSON.stringify(fields))) throw Error("The Permit2 authorization is invalid.");
  const m = data.message;
  if (!sameAddress(m.permitted.token, USDT_ADDRESS) || m.permitted.amount !== offer.amount_units || !sameAddress(m.spender, EXACT_PROXY) || !isAddress(m.witness.to, {strict:false}) || !sameAddress(m.witness.to, merchant.recipient) || !uint(m.nonce) || !uint(m.deadline) || !uint(m.witness.validAfter) || Number(m.deadline) !== quote.expires_at || Number(m.deadline) <= now || Number(m.deadline) > now+120 || Number(m.witness.validAfter) > now || BigInt(m.deadline)-BigInt(m.witness.validAfter) > 125n) throw Error("The Permit2 authorization differs from the quote or has expired.");
}
export async function signPaymentQuote(wallet: ConnectedWallet, quote: PaymentQuote, merchant: Merchant, owner: string, perCall: string, dailyCap: string) {
  validatePaymentQuote(quote,merchant,owner,perCall,dailyCap);
  if (!quote.typed_data) throw Error("Approve USDT and request a fresh quote before signing payment.");
  await bnbWalletClient(wallet, owner);
  // Validate again after a chain switch, since a short quote can expire while the wallet is open.
  validatePaymentQuote(quote,merchant,owner,perCall,dailyCap);
  const provider = await wallet.getEthereumProvider();
  return await provider.request({method:"eth_signTypedData_v4",params:[owner,JSON.stringify(quote.typed_data)]}) as Hex;
}
