import type { RuntimeInput } from "./api";

type CreationRequest = { key: string; body: string };
const requests = new Map<string, CreationRequest>();
const storageKey = (owner: string) => `tab:account:create:${owner}`;

/** A lost response must recover the same draft, including after a page reload. */
export function creationRequest(owner: string, input: RuntimeInput): string {
  const body = JSON.stringify(input);
  let previous = requests.get(owner);
  if (!previous) {
    try {
      const saved: unknown = JSON.parse(localStorage.getItem(storageKey(owner)) || "null");
      if (saved && typeof saved === "object" && "key" in saved && "body" in saved &&
        typeof saved.key === "string" && /^[a-zA-Z0-9-]{16,64}$/.test(saved.key) && typeof saved.body === "string") {
        previous = { key: saved.key, body: saved.body };
      }
    } catch { /* Keep retry protection in memory when browser storage is disabled. */ }
  }
  const request = previous?.body === body ? previous : { key: crypto.randomUUID(), body };
  requests.set(owner, request);
  try { localStorage.setItem(storageKey(owner), JSON.stringify(request)); } catch { /* See above. */ }
  return request.key;
}

export function finishCreation(owner: string): void {
  requests.delete(owner);
  try { localStorage.removeItem(storageKey(owner)); } catch { /* Browser storage may be disabled. */ }
}
