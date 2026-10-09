import { createContext, useContext } from "react";
import { createWalletClient, custom, getAddress, isAddress, type Hex } from "viem";
import { bsc } from "viem/chains";
import type { ConnectedWallet } from "@privy-io/react-auth";

export const BNB_CHAIN_ID = 56;
export const USDT_ADDRESS = "0x55d398326f99059fF775485246999027B3197955";
export const USDT_DECIMALS = 18;
export const ChainContext = createContext<number>(BNB_CHAIN_ID);
export const useChainId = () => useContext(ChainContext);
export const sameAddress = (a: string | null | undefined, b: string | null | undefined) => Boolean(a && b && a.toLowerCase() === b.toLowerCase());
export const shortId = (value: string | null | undefined) => value && value.length > 16 ? `${value.slice(0, 8)}…${value.slice(-4)}` : value || "unregistered";
export function explorer(value: string, type: "tx" | "address" = "tx", chainId = BNB_CHAIN_ID) {
  const valid = type === "tx" ? /^0x[0-9a-fA-F]{64}$/.test(value) : isAddress(value, { strict: false });
  return chainId === BNB_CHAIN_ID && valid ? `https://bscscan.com/${type}/${value}` : undefined;
}
export type EvmTransaction = { to: string; data: string; value: string; chainId: string | number };
export function validateTransaction(value: EvmTransaction) {
  if (Number(value.chainId) !== BNB_CHAIN_ID) throw Error("This transaction must use BNB Smart Chain (56).");
  if (!isAddress(value.to, { strict: false }) || !/^0x(?:[0-9a-fA-F]{2})*$/.test(value.data)) throw Error("The wallet transaction is invalid.");
  if (!/^(?:0x[0-9a-fA-F]+|\d+)$/.test(value.value)) throw Error("The BNB transaction amount is invalid.");
  const amount = BigInt(value.value);
  if (amount < 0n || amount >= 2n ** 256n) throw Error("The BNB transaction amount is invalid.");
  return { to: getAddress(value.to), data: value.data as Hex, value: amount };
}
export function matchesTransaction(transaction: Record<string, unknown>, expected:EvmTransaction, sender:string) {
  try {
    const request=validateTransaction(expected);
    return Number(transaction.chainId)===BNB_CHAIN_ID && typeof transaction.from==="string" && sameAddress(transaction.from,sender) && typeof transaction.to==="string" && sameAddress(transaction.to,request.to) && typeof transaction.input==="string" && transaction.input.toLowerCase()===request.data.toLowerCase() && typeof transaction.value==="string" && BigInt(transaction.value)===request.value;
  } catch {return false;}
}
export async function bnbWalletClient(wallet: ConnectedWallet, expectedAddress?: string) {
  if (!isAddress(wallet.address, { strict: false }) || (expectedAddress && !sameAddress(wallet.address, expectedAddress))) throw Error("Connect the wallet that owns this agent.");
  await wallet.switchChain(BNB_CHAIN_ID);
  const provider = await wallet.getEthereumProvider();
  if (Number(await provider.request({ method: "eth_chainId" })) !== BNB_CHAIN_ID) throw Error("Switch your wallet to BNB Smart Chain (56).");
  const accounts = await provider.request({ method: "eth_accounts" }) as string[];
  if (!accounts.some(address => sameAddress(address, wallet.address))) throw Error("The connected wallet account changed. Connect the agent owner again.");
  return createWalletClient({ account: getAddress(wallet.address), chain: bsc, transport: custom(provider) });
}
/** Only created by code that has not invoked the wallet's transaction submission. */
export class NoWalletSubmissionError extends Error {
  constructor(cause: unknown) {
    super(cause instanceof Error ? cause.message : "The wallet could not prepare this transaction.", { cause });
    this.name = "NoWalletSubmissionError";
  }
}
export async function sendBnbTransaction(wallet: ConnectedWallet, transaction: EvmTransaction, expectedAddress?: string) {
  let request: ReturnType<typeof validateTransaction>;
  let client: Awaited<ReturnType<typeof bnbWalletClient>>;
  try {
    request = validateTransaction(transaction);
    client = await bnbWalletClient(wallet, expectedAddress);
  } catch (cause) {
    throw new NoWalletSubmissionError(cause);
  }
  // Submission failures can occur after broadcast. Never mark this call as unsent.
  return client.sendTransaction(request);
}
export async function waitForTransaction(hash: string) {
  if (!/^0x[0-9a-fA-F]{64}$/.test(hash)) throw Error("The transaction hash is invalid.");
  for (let attempt = 0; attempt < 60; attempt++) {
    const response = await fetch(`/api/bnb/transactions/${hash}`, { signal: AbortSignal.timeout(15000) });
    const result = await response.json();
    if (!response.ok) throw Error(result.detail || "BNB Smart Chain confirmation is unavailable.");
    if (result.status === "failed") throw Error("The BNB Smart Chain transaction failed.");
    if (result.status === "confirmed") return;
    await new Promise(resolve => setTimeout(resolve, 1500));
  }
  throw Error("The transaction is still pending. Refresh its confirmation before sending another.");
}
