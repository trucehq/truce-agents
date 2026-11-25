#!/usr/bin/env node
// TruceHQ as an ATDRP-0.1 provider for the ACP SEP #303 conformance runner.
// Reads one case input (JSON) on stdin, sends it to TruceHQ's production checker, prints its JSON.
// Node standard library only (Node 18+). The checker is stateless and stores nothing.
//
//   DISPUTE_PROVIDER_CMD="node atdrp-provider.mjs" node scripts/run-dispute-conformance.js \
//     examples/unreleased/dispute_resolution/conformance_cases.json
//
// TRUCE_ATDRP_URL overrides the endpoint (default https://trucehq.com/api/atdrp/check).
const url = process.env.TRUCE_ATDRP_URL || "https://trucehq.com/api/atdrp/check";
let input = "";
for await (const chunk of process.stdin) input += chunk;
const res = await fetch(url, {
  method: "POST",
  headers: { "Content-Type": "application/json", "User-Agent": "trucehq-atdrp-conformance/1" },
  body: input,
});
const text = await res.text();
if (!res.ok && !text.trim().startsWith("{")) {
  console.error(`HTTP ${res.status}`);
  process.exit(1);
}
process.stdout.write(text);
