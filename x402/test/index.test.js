import { test } from "node:test";
import assert from "node:assert/strict";
import { disputeExtension, readSettlement, readDisputeTerms, withReceipts, openPaymentDispute, answerPaymentDispute } from "../src/index.js";

const b64 = (o) => Buffer.from(JSON.stringify(o)).toString("base64");
const settle = { success: true, transaction: "0xabc", network: "eip155:8453", payer: "0x857b" };

test("extension shape", () => {
  const e = disputeExtension({ contact: "ops@seller.ai", notifyUrl: "https://seller.ai/truce" });
  assert.equal(e.dispute.provider, "trucehq");
  assert.equal(e.dispute.notify, "https://seller.ai/truce");
  assert.throws(() => disputeExtension({}));
});

test("reads v2 and v1 settlement headers", () => {
  assert.deepEqual(readSettlement(new Response("", { headers: { "PAYMENT-RESPONSE": b64(settle) } })), settle);
  assert.deepEqual(readSettlement(new Response("", { headers: { "X-PAYMENT-RESPONSE": b64(settle) } })), settle);
  assert.equal(readSettlement(new Response("")), null);
});

test("reads dispute terms from 402 header", async () => {
  const pr = { x402Version: 2, accepts: [], extensions: disputeExtension({ contact: "a@b.co" }) };
  const res = new Response("", { status: 402, headers: { "PAYMENT-REQUIRED": b64(pr) } });
  assert.equal((await readDisputeTerms(res)).contact, "a@b.co");
});

test("keeps receipts of paid calls only", async () => {
  const f = withReceipts(async (u) => new Response("ok", { headers: u.includes("paid") ? { "PAYMENT-RESPONSE": b64(settle) } : {} }));
  await f("https://x.ai/paid");
  await f("https://x.ai/free");
  assert.equal(f.receipts.length, 1);
  assert.equal(f.receipts[0].transaction, "0xabc");
});

test("open requires authorizedBy and notifies seller", async () => {
  const calls = [];
  const fake = async (url, init) => {
    calls.push([url, JSON.parse(init.body)]);
    return new Response(JSON.stringify({ invite_code: "inv123", key: "tak_x" }), { status: 201 });
  };
  const receipt = { ...settle, url: "https://x.ai/paid", status: 500, at: "now" };
  await assert.rejects(() => openPaymentDispute({ receipt, amountUsd: 12, statement: "s", wants: "w", fetchImpl: fake }));
  const out = await openPaymentDispute({
    receipt, amountUsd: 12, statement: "Paid, got a 500", wants: "Refund $12", authorizedBy: "Sam Lee",
    sellerTerms: { notify: "https://seller.ai/truce", refundPolicy: "Refund on 5xx" }, fetchImpl: fake,
  });
  assert.equal(out.invite_code, "inv123");
  assert.equal(out.seller_notified, true);
  assert.equal(calls[0][1].authorization.by, "Sam Lee");
  assert.match(calls[0][1].evidence, /0xabc/);
  assert.equal(calls[1][1].invite_code, "inv123");
});

test("seller answers via callback", async () => {
  const fake = async (url, init) => new Response(JSON.stringify({ status: "proposed", body: JSON.parse(init.body) }), { status: 201 });
  const out = await answerPaymentDispute({ invite_code: "inv123", transaction: "0xabc", network: "eip155:8453" },
    async () => ({ statement: "We served it", wants: "No refund", authorizedBy: "Seller ops" }), { fetchImpl: fake });
  assert.equal(out.answered, true);
  assert.equal(out.body.authorization.by, "Seller ops");
});
