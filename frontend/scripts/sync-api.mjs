import { writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";

const origin = process.env.TAB_API_ORIGIN ?? "http://127.0.0.1:4297";
const schemaUrl = new URL("/api/openapi.json", origin);
const response = await fetch(schemaUrl);
if (!response.ok) throw new Error(`OpenAPI download failed (${response.status}).`);
const schema = await response.json();
if (!schema.openapi || !schema.paths || !schema.components?.schemas) {
  throw new Error("The API did not return a complete OpenAPI schema.");
}
await writeFile(new URL("../src/lib/openapi.json", import.meta.url), `${JSON.stringify(schema, null, 2)}\n`);
execFileSync(process.execPath, ["node_modules/openapi-typescript/bin/cli.js", "src/lib/openapi.json", "-o", "src/lib/api-schema.ts"], { stdio: "inherit" });
