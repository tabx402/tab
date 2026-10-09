import { chromium } from "../node_modules/playwright/index.mjs";
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
process.chdir(fileURLToPath(new URL("../..", import.meta.url)));
mkdirSync("qa", { recursive: true });
const origin = process.env.TAB_TEST_ORIGIN || "http://127.0.0.1:5197";
const browser = await chromium.launch({ headless: true });
const errors = [];
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
page.on("pageerror", (e) => errors.push(e.message));
await page.goto(origin, { waitUntil: "networkidle" });
await page.locator(".live-activity").waitFor();
assert.equal(await page.locator(".mode-control").count(), 0);
await page.screenshot({ path: "qa/live-overview.png", fullPage: true });
await page.goto(`${origin}/agents`, { waitUntil: "networkidle" });
const response = await page.request.get(`${origin}/api/agents/live`);
assert.equal(response.status(), 200);
const agents = await response.json();
assert.equal(
  await page.locator(".agents-panel tbody tr").count(),
  agents.length,
);
if (agents.length) {
  await page.locator(`a[href="/agents/${agents[0].id}"]`).click();
  await page
    .getByRole("heading", { name: agents[0].name, exact: true })
    .waitFor();
}
await page.goto(`${origin}/activity`, { waitUntil: "networkidle" });
await page
  .getByRole("combobox", { name: "Filter live activity" })
  .selectOption("payments");
assert.equal(await page.locator(".registration-log").count(), 0);
await page
  .getByRole("combobox", { name: "Filter live activity" })
  .selectOption("all");
await page
  .getByRole("textbox", { name: "Search live activity" })
  .fill("nothing-matches-this");
assert.equal(await page.locator(".readable-log>.readable-log-row").count(), 0);
await page.getByRole("textbox", { name: "Search live activity" }).fill("");
const downloadPromise = page.waitForEvent("download");
await page.getByRole("button", { name: "export", exact: true }).click();
assert.equal((await downloadPromise).suggestedFilename(), "tab-activity.json");
await page.screenshot({ path: "qa/live-log.png", fullPage: true });
await page.goto(`${origin}/account`, { waitUntil: "networkidle" });
assert.equal(await page.locator("footer").count(), 0);
await page.getByRole("button", { name: "connect wallet", exact: true }).click();
await page.getByText("MetaMask", { exact: true }).waitFor();
assert.equal(
  (await page.request.get(`${origin}/api/account/runtime`)).status(),
  401,
);
for (const width of [320, 390, 768]) {
  const mobile = await browser.newPage({
    viewport: { width, height: 844 },
    isMobile: width < 500,
  });
  mobile.on("pageerror", (e) => errors.push(e.message));
  for (const route of [
    "/",
    "/agents",
    "/providers",
    "/activity",
    "/account",
    "/protocol",
  ]) {
    await mobile.goto(`${origin}${route}`, { waitUntil: "networkidle" });
    await mobile.locator("h1").waitFor();
    assert.equal(
      await mobile.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
      false,
      `${width} ${route} overflows`,
    );
  }
  await mobile.close();
}
assert.deepEqual(errors, []);
console.log(
  "Live overview, directory, profiles, log filters/export, login, auth boundary and mobile layouts passed.",
);
await browser.close();
