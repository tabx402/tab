import type { components } from "./api-schema";
export type Provider = components["schemas"]["Provider"];
export type AgentPlan = components["schemas"]["AgentPlan"];
export type AgentPlanInput = components["schemas"]["AgentPlanInput"];
export type Config = components["schemas"]["PublicConfig"];
export const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);
export async function request<T>(
  path: string,
  options: RequestInit = {},
  token?: string | null,
): Promise<T> {
  const response = await fetch(`/api${path}`, {
    ...options,
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...options.headers,
    },
  });
  if (!response.ok) {
    const body = await response.json().catch(() => null);
    throw new Error(
      typeof body?.detail === "string"
        ? body.detail
        : `Request failed (${response.status}).`,
    );
  }
  return response.status === 204
    ? (undefined as T)
    : (response.json() as Promise<T>);
}
export const money = (value: number) =>
  new Intl.NumberFormat("en-US", {
    maximumFractionDigits: 2,
    minimumFractionDigits: 2,
  }).format(value);
export type RegistryData = components["schemas"]["RegistryData"];
export type RuntimeAgent = components["schemas"]["RuntimeAgent"];
export type RuntimeInput = components["schemas"]["AgentInput"];
export type AgentEvent = components["schemas"]["AgentEvent"];
export type AgentRun = components["schemas"]["AgentRun"];
export type PurchaseQuote = components["schemas"]["PurchaseQuote"];
export type PurchaseResult = components["schemas"]["PurchaseResult"];

export type SponsorshipResult = components["schemas"]["SponsorshipResult"];
export type SponsoredChallenge = components["schemas"]["SponsoredChallenge"];
export type Job = components["schemas"]["Job"];
export type JobInput = components["schemas"]["JobInput"];
export type PublicJob = components["schemas"]["PublicJob"];
export type JobSystem = components["schemas"]["JobSystem"];
export type JobIntent = components["schemas"]["JobIntent"];
export type JobActionRecord = components["schemas"]["JobActionRecord"];
export type JobMerchant = components["schemas"]["JobMerchant"];
export type ModelOption = { id: string; name: string; provider: string; available: boolean; status: string; billing: string; max_output_tokens: number; catalog_verified?: boolean };
export type FeedItem = AgentEvent & { preview?: Record<string, unknown> | null };
export type FeedData = { mode: "public"; items: FeedItem[]; agents: components["schemas"]["PublicAgent"][]; jobs: PublicJob[]; network: string; chain_id: number };
export type Bounty = components["schemas"]["Bounty"];
export type BountyInput = components["schemas"]["BountyInput"];
export type WalletActionInput = components["schemas"]["FinancialInput"];
export type WalletActionIntent = components["schemas"]["TransactionIntent"];
export type WorkMetrics = { currency: "USDT"; source: string; bonded_agents_earning_payments: number; completed_commitments: number; missed_commitments: number; paid_collaborations: number; customer_payments_usdt: string; fees_collected_usdt: string; buyback_usdt_spent: string; buyback_tokens_acquired: string; settlement_status: string };
export type TokenSystem = { official_tab_address: string | null; status: string; staking_enabled: boolean; paired_token_required: boolean; tab_holding_required: boolean; fee_destination: string; fee_bps: number; fee_status: string; penalty_rule: string; buyer_disagreement_slashable: boolean; token_bond_guarantees_usdt: boolean; trading_pause_scope?: string };
export type BackingAsset = { symbol: string; name: string; address: string | null; decimals: number; token_standard: string; category: "usdt" | "native" | "tokenized_stock" | "token"; enabled: boolean; status: string; network: string; chain_id: number };
