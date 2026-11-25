import { test } from "node:test";
import assert from "node:assert/strict";
import { truceActionProvider } from "../src";

type Call = { url: string; init: RequestInit };

// AgentKit's decorator also posts analytics via fetch; record only TruceHQ calls.
function mockFetch(reply: unknown) {
  const calls: Call[] = [];
  globalThis.fetch = (async (url: string, init: RequestInit) => {
    if (String(url).startsWith("https://trucehq.com/")) calls.push({ url: String(url), init });
    return new Response(JSON.stringify(reply), { status: 200, headers: { "content-type": "application/json" } });
  }) as typeof fetch;
  return calls;
}

const wallet = {} as never; // Truce actions never use the wallet.
const actions = truceActionProvider().getActions(wallet);
const action = (suffix: string) => actions.find((a) => a.name.endsWith(suffix))!;

test("provider exposes four actions and supports any network", () => {
  assert.deepEqual(
    actions.map((a) => a.name).sort(),
    [
      "TruceActionProvider_decide_agent_dispute",
      "TruceActionProvider_get_agent_dispute",
      "TruceActionProvider_open_agent_dispute",
      "TruceActionProvider_respond_agent_dispute",
    ],
  );
  assert.equal(truceActionProvider().supportsNetwork({ protocolFamily: "evm", networkId: "base-mainnet" }), true);
});

test("open_agent_dispute posts the expected body incl. onchain tx hash in refs.payment_id", async () => {
  const calls = mockFetch({ key: "k_secret", invite_code: "INV123" });
  const args = action("open_agent_dispute").schema.parse({
    topic: "Undelivered API credits",
    name: "Alice",
    acting_for: "Alice (buyer agent)",
    statement: "Paid 25 USDC on Base, credits never delivered.",
    wants: "Refund 25 USDC",
    mandate: { role: "payee", limit: 20, currency: "usd", auto_accept: true },
    refs: { payment_id: "0x5f2a9c0b7e1d4a3f8b6c2e9d1a7f4b3c8e2d6a9f1b4c7e0d3a6f9b2c5e8d1a4f" },
    authorized: true,
  });
  const out = await action("open_agent_dispute").invoke(args);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, "https://trucehq.com/api/agent-disputes");
  assert.equal(calls[0].init.method, "POST");
  assert.deepEqual(JSON.parse(String(calls[0].init.body)), args);
  assert.equal(JSON.parse(out).invite_code, "INV123");
});

test("schema rejects authorized=false", () => {
  const r = action("open_agent_dispute").schema.safeParse({
    topic: "t", name: "n", statement: "s", wants: "w", authorized: false,
  });
  assert.equal(r.success, false);
});

test("respond, decide and get hit the right endpoints", async () => {
  const calls = mockFetch({ key: "k2", status: "proposed" });
  await action("respond_agent_dispute").invoke({ invite_code: "INV123", name: "Bob", statement: "Delivered", wants: "Keep payment", authorized: true });
  await action("decide_agent_dispute").invoke({ key: "k2", decision: "accept" });
  await action("get_agent_dispute").invoke({ key: "k2" });
  assert.equal(calls[0].url, "https://trucehq.com/api/agent-disputes/respond");
  assert.equal(JSON.parse(String(calls[0].init.body)).invite_code, "INV123");
  assert.deepEqual(JSON.parse(String(calls[1].init.body)), { key: "k2", decision: "accept" });
  assert.equal(calls[2].url, "https://trucehq.com/api/agent-disputes/k2");
  assert.equal(calls[2].init.method, "GET");
});
