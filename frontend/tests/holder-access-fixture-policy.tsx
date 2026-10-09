import type { ReactNode } from "react";
import { HolderAccessContext, holderPermitted, holderRecoveryAction, type HolderGate, type HolderStatus } from "../src/lib/holder-access";

// Existing isolated component tests run against an explicitly disabled rollout.
// Production's missing-provider default stays closed to new actions.
const blocked = new URLSearchParams(location.search).get("holder") === "blocked";
const policy = { status: blocked ? "not_holder" : "disabled", enforced: blocked, eligible: false } as HolderStatus;
const access: HolderGate = { allows: (action, details) => holderRecoveryAction(action, details) || holderPermitted(policy), require: async (action, details) => { if (!holderRecoveryAction(action, details) && !holderPermitted(policy)) throw Error("A TAB holding is required for this new action."); } };
export function FixtureHolderAccess({ children }: { children: ReactNode }) {
  return <HolderAccessContext.Provider value={access}>{children}</HolderAccessContext.Provider>;
}
