export const USDT_SCALE = 10n ** 18n;
// Monetary API values stay decimal strings, including amounts below one token.
// Reject excess precision instead of rounding before a wallet request.
export function usdtUnits(value: string): bigint | null {
  const match = /^(\d+)(?:\.(\d{0,18}))?$/.exec(value);
  if (!match) return null;
  const units = BigInt(match[1]) * USDT_SCALE + BigInt((match[2] ?? "").padEnd(18, "0"));
  return units < 2n ** 256n ? units : null;
}
export function amountFromUnits(units: bigint, minimumDecimals = 0): string {
  const negative = units < 0n;
  const absolute = negative ? -units : units;
  const fraction = (absolute % USDT_SCALE).toString().padStart(18, "0").replace(/0+$/, "").padEnd(minimumDecimals, "0");
  return `${negative ? "-" : ""}${absolute / USDT_SCALE}${fraction ? `.${fraction}` : ""}`;
}
export function formatAmount(value: string | number | undefined, minimumDecimals = 0): string {
  if (value === undefined) return "—";
  const units = usdtUnits(String(value));
  if (units === null) return "—";
  const [integer, fraction] = amountFromUnits(units, minimumDecimals).split(".");
  return `${BigInt(integer).toLocaleString("en-US")}${fraction ? `.${fraction}` : ""}`;
}
export function minimumAmount(...values: string[]): string {
  const units = values.map(value => usdtUnits(value));
  if (units.some(value => value === null)) return "0";
  return amountFromUnits((units as bigint[]).reduce((lowest, value) => value < lowest ? value : lowest));
}
