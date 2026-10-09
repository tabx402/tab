import { Coins, Wallet } from "lucide-react";
import "./feature-flow.css";

export function OnboardingCosts({ sponsored = false }: { sponsored?: boolean }) {
  return <aside className="onboarding-costs" aria-label="Network and costs before you start">
    <div><Wallet size={18} /><span>BNB mainnet<strong>{sponsored ? "registration gas sponsored" : "BNB pays registration gas"}</strong></span></div>
    <div><Coins size={18} /><span>USDT is your spending budget<strong>read-only chain checks spend 0 USDT</strong></span></div>
    <p>Saving a setup is free. Paid requests show their price and need wallet approval or a spending permission you granted.</p>
  </aside>;
}
