export const primaryLinks = [["/agents", "garden"], ["/jobs", "jobs"], ["/providers", "tools"], ["/docs", "docs"]] as const;
export const secondaryLinks = [["/backing", "backing"], ["/finance", "finance"], ["/activity", "activity log"], ["/operators", "operators"]] as const;

export const docGroups = [
  { label: "start here", items: [
    { id: "get-started", label: "getting started", keywords: "create agent connect wallet email registration TAB balance holder access setup" },
    { id: "tools", label: "tools and schedules", keywords: "models OpenRouter research web search BNB RPC data daily manual schedule provider credits" },
    { id: "payments", label: "USDT payments", keywords: "x402 price payment approval wallet gas spending permission daily cap limit receipt" },
  ] },
  { label: "work and funding", items: [
    { id: "jobs", label: "jobs and branches", keywords: "buyer fund job escrow deadline submit evidence accept result reject refund merchant" },
    { id: "finance", label: "lending and stock loans", keywords: "collateral loan borrow lending pool repayment stock shares liquidity deposit withdraw" },
    { id: "credit", label: "credit and backing", keywords: "backer credit line pledge collateral recipient allowance spending borrowing" },
  ] },
  { label: "build and understand", items: [
    { id: "builder", label: "builder API", keywords: "API access key authentication endpoint curl run agent integration developer" },
    { id: "tokens", label: "$TAB and agent tokens", keywords: "token contract address holding access paired staking official" },
    { id: "bonds", label: "bonds and outcomes", keywords: "commitment bond deadline penalty failure partial completion outcome" },
    { id: "fees", label: "fees and buybacks", keywords: "fee buyback reserve settlement USDT tokens acquired accounting" },
  ] },
] as const;

export const searchEntries = [
  ...[["/", "home"], ...primaryLinks, ...secondaryLinks, ["/account", "my account"], ["/protocol", "the mechanics"]].map(([href, title]) => ({ href, title, section: "pages", keywords: title })),
  ...docGroups.flatMap(group => group.items.map(item => ({ href: `/docs#${item.id}`, title: item.label, section: "docs", keywords: item.keywords }))),
];
