import { test } from "node:test";
import assert from "node:assert/strict";
import { trucePlugin } from "../src/index.js";

type Call = { url: string; init: RequestInit };

function mockFetch(reply: unknown) {
  const calls: Call[] = [];
  globalThis.fetch = (async (url: string, init: RequestInit) => {
    calls.push({ url, init });
    return new Response(JSON.stringify(reply), { status: 200, headers: { "content-type": "application/json" } });
  }) as typeof fetch;
  return calls;
}

const runtime = { getSetting: () => null } as never;
const message = { content: { text: "" } } as never;
const action = (name: string) => trucePlugin.actions!.find((a) => a.name === name)!;

test("plugin exposes the four actions", () => {
  assert.deepEqual(
    trucePlugin.actions!.map((a) => a.name),
    ["OPEN_AGENT_DISPUTE", "RESPOND_AGENT_DISPUTE", "DECIDE_AGENT_DISPUTE", "GET_AGENT_DISPUTE"],
  );
});

test("OPEN_AGENT_DISPUTE posts the expected body", async () => {
  const calls = mockFetch({ key: "k_secret", invite_code: "INV123" });
  const parameters = {
    topic: "Refund for undelivered order",
    name: "Alice",
    acting_for: "Alice (buyer)",
    statement: "Paid 40 USDC, nothing arrived.",
    wants: "Full refund",
    evidence: "order 991",
    mandate: { role: "payee", limit: 30, currency: "usd", auto_accept: true },
    refs: { order_id: "991", payment_id: "0xabc" },
    authorized: true,
  };
  const r = await action("OPEN_AGENT_DISPUTE").handler(runtime, message, undefined, { parameters });
  assert.equal(r.success, true);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, "https://trucehq.com/api/agent-disputes");
  assert.equal(calls[0].init.method, "POST");
  assert.deepEqual(JSON.parse(String(calls[0].init.body)), parameters);
  assert.match(r.text!, /INV123/);
});

test("OPEN_AGENT_DISPUTE refuses without authorized=true and makes no request", async () => {
  const calls = mockFetch({});
  const r = await action("OPEN_AGENT_DISPUTE").handler(runtime, message, undefined, {
    parameters: { topic: "t", name: "n", statement: "s", wants: "w" },
  });
  assert.equal(r.success, false);
  assert.equal(calls.length, 0);
});

test("RESPOND, DECIDE and GET hit the right endpoints", async () => {
  const calls = mockFetch({ key: "k2", status: "proposed" });
  await action("RESPOND_AGENT_DISPUTE").handler(runtime, message, undefined, {
    parameters: { invite_code: "INV123", name: "Bob", statement: "Shipped", wants: "Keep payment", authorized: true },
  });
  await action("DECIDE_AGENT_DISPUTE").handler(runtime, message, undefined, {
    parameters: { key: "k2", decision: "accept", note: "ok" },
  });
  await action("GET_AGENT_DISPUTE").handler(runtime, message, undefined, { parameters: { key: "k2" } });
  assert.equal(calls[0].url, "https://trucehq.com/api/agent-disputes/respond");
  assert.equal(JSON.parse(String(calls[0].init.body)).invite_code, "INV123");
  assert.equal(calls[1].url, "https://trucehq.com/api/agent-disputes/decide");
  assert.deepEqual(JSON.parse(String(calls[1].init.body)), { key: "k2", decision: "accept", note: "ok" });
  assert.equal(calls[2].url, "https://trucehq.com/api/agent-disputes/k2");
  assert.equal(calls[2].init.method, "GET");
});
