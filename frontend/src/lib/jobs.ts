import { formatAmount } from "./amounts";
import { request } from "./api";
import type { Job, JobInput, JobIntent, JobActionRecord, PublicJob, JobSystem, JobMerchant } from "./api";

export type AccountAPI = <T>(path: string, options?: RequestInit) => Promise<T>;
export type JobRun = {
  id: string; job_id: string; agent_id: string; status: string; started_at: string; finished_at: string | null;
  output: { job_id: string; terms_hash: string; task: string; tools: Record<string, unknown>; result_hash?: string; evidence_hash?: string; evidence_status: "attached" | "not_attached"; error?: string };
};
const post = (body?: unknown): RequestInit => ({ method: "POST", ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
export const jobAPI = {
  public: () => request<PublicJob[]>("/jobs"),
  system: () => request<JobSystem>("/jobs/system"),
  merchants: () => request<JobMerchant[]>("/jobs/services"),
  list: (api: AccountAPI, agent: string) => api<Job[]>(`/account/jobs?agent_id=${encodeURIComponent(agent)}`),
  actions: (api: AccountAPI) => api<JobActionRecord[]>("/account/job-actions"),
  create: (api: AccountAPI, agent: string, input: JobInput) => api<Job>(`/account/runtime/${agent}/jobs`, post(input)),
  branch: (api: AccountAPI, parent: string, input: JobInput) => api<Job>(`/account/jobs/${parent}/branches`, post(input)),
  evidence: (api: AccountAPI, id: string) => api<Job>(`/account/jobs/${id}/evidence`, post()),
  attach: (api: AccountAPI, id: string, evidence: Record<string, unknown>) => api<Job>(`/account/jobs/${id}/evidence`, { method: "PUT", body: JSON.stringify(evidence) }),
  cancelDraft: (api: AccountAPI, id: string) => api<Job>(`/account/jobs/${id}/cancel-draft`, post()),
  prepare: async (api: AccountAPI, id: string, action: string) => { const intent=await api<JobIntent>(`/account/jobs/${id}/prepare`, post({ action })); window.dispatchEvent(new Event("tab:jobs-changed")); return intent; },
  confirm: async (api: AccountAPI, id: string, tx_hash: string) => { const job=await api<Job>(`/account/job-actions/${id}/confirm`, post({ tx_hash })); window.dispatchEvent(new Event("tab:jobs-changed")); window.dispatchEvent(new Event("tab:wallet-actions-changed")); return job; },
  refresh: (api: AccountAPI, id: string) => api<Job>(`/account/jobs/${id}/refresh`, post()),
  run: (api: AccountAPI, id: string) => api<JobRun>(`/account/jobs/${id}/run`, post()),
  runs: (api: AccountAPI, id: string, options?: RequestInit) => api<JobRun[]>(`/account/jobs/${id}/runs`, options),
};
export function jobMoney(value: string | number): string {
  return formatAmount(value);
}
