// @trucehq/x402: a neutral dispute route for x402 payments.
// Sellers advertise it in their 402 response; buyers keep a receipt of every paid call and can
// open a Truce when a paid call went wrong. Truce never holds funds: it hears both agents in
// private, proposes something fair, and the humans (or their mandates) decide. Free for agents.

const TRUCE = "https://trucehq.com";

/** For sellers: put this under `extensions` in your PaymentRequired (402) response. */
export function disputeExtension({ contact, notifyUrl, refundPolicy = "", provider = TRUCE } = {}) {
  if (!contact && !notifyUrl) throw new Error("disputeExtension needs a contact (email or URL) or a notifyUrl");
  return {
    dispute: {
      provider: "trucehq",
      open: `${provider.replace(/\/$/, "")}/api/agent-disputes`,
      ...(notifyUrl ? { notify: notifyUrl } : {}),
      ...(contact ? { contact } : {}),
      ...(refundPolicy ? { refundPolicy } : {}),
      docs: "https://trucehq.com/agents",
    },
  };
}

const b64json = (s) => {
  try {
    const txt = typeof atob === "function" ? atob(s) : Buffer.from(s, "base64").toString("utf8");
    return JSON.parse(txt);
  } catch {
    return null;
  }
};

/** Reads the settlement receipt from an x402 response (v2 PAYMENT-RESPONSE, v1 X-PAYMENT-RESPONSE). */
export function readSettlement(res) {
  const h = res.headers.get("PAYMENT-RESPONSE") || res.headers.get("X-PAYMENT-RESPONSE");
  return h ? b64json(h) : null;
}

/** Reads the dispute extension a seller advertised in its 402 (PAYMENT-REQUIRED header or JSON body). */
export async function readDisputeTerms(res402) {
  let pr = null;
  const h = res402.headers.get("PAYMENT-REQUIRED");
  if (h) pr = b64json(h);
  if (!pr) {
    try {
      pr = await res402.clone().json();
    } catch {
      pr = null;
    }
  }
  return pr?.extensions?.dispute ?? null;
}

/**
 * For buyers: wraps a fetch that already pays x402 (for example from @x402/fetch).
 * Every paid call is kept as a receipt: url, status, transaction, network, payer, time.
 * Nothing leaves your process unless you open a dispute.
 */
export function withReceipts(paidFetch, { keep = 200 } = {}) {
  const receipts = [];
  const f = async (input, init) => {
    const res = await paidFetch(input, init);
    const s = readSettlement(res);
    if (s?.success) {
      receipts.unshift({
        url: typeof input === "string" ? input : input.url,
        status: res.status,
        transaction: s.transaction,
        network: s.network,
        payer: s.payer,
        at: new Date().toISOString(),
      });
      receipts.length = Math.min(receipts.length, keep);
    }
    return res;
  };
  f.receipts = receipts;
  return f;
}

/**
 * Opens a Truce about a paid x402 call. `authorizedBy` is required: the person or role who asked
 * your agent to act. `mandate` is private (never shown to the seller): e.g. { min_refund_usd: 5 }.
 * Returns { invite_code, key, ... }. If the seller advertised a notify URL, the invite is posted there.
 */
export async function openPaymentDispute({
  receipt,
  amountUsd,
  statement,
  wants,
  authorizedBy,
  scope = "x402 payment dispute",
  name = "Buyer agent",
  actingFor = "",
  sellerTerms = null,
  mandate = {},
  agentKey,
  base = TRUCE,
  fetchImpl = fetch,
}) {
  if (!authorizedBy || !String(authorizedBy).trim())
    throw new Error("authorizedBy is required: name who asked you to open this dispute.");
  if (!receipt?.transaction) throw new Error("receipt with a transaction is required");
  const evidence = [
    `x402 payment: tx ${receipt.transaction} on ${receipt.network}, payer ${receipt.payer}.`,
    `Resource: ${receipt.url} (HTTP ${receipt.status}) at ${receipt.at}.`,
    sellerTerms?.refundPolicy ? `Seller's published refund policy: ${sellerTerms.refundPolicy}` : "",
  ]
    .filter(Boolean)
    .join("\n");
  const r = await fetchImpl(`${base.replace(/\/$/, "")}/api/agent-disputes`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(agentKey ? { Authorization: `Bearer ${agentKey}` } : {}),
    },
    body: JSON.stringify({
      topic: `x402 payment of $${amountUsd} for ${receipt.url}`.slice(0, 160),
      name,
      acting_for: actingFor,
      statement,
      wants,
      evidence,
      category: "payment",
      currency: "usd",
      mandate,
      authorized: true,
      authorization: { by: String(authorizedBy).trim(), scope },
    }),
  });
  const out = await r.json();
  if (!r.ok) throw new Error(out?.error || `Truce error ${r.status}`);
  if (sellerTerms?.notify && out.invite_code) {
    try {
      await fetchImpl(sellerTerms.notify, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          provider: "trucehq",
          invite_code: out.invite_code,
          transaction: receipt.transaction,
          network: receipt.network,
          respond: `${base.replace(/\/$/, "")}/api/agent-disputes/respond`,
        }),
      });
      out.seller_notified = true;
    } catch {
      out.seller_notified = false;
    }
  }
  return out;
}

/**
 * For sellers: handle the notify call. `decide` gets the invite and returns your side
 * ({ statement, wants, evidence?, mandate?, authorizedBy }) or null to answer later by hand.
 */
export async function answerPaymentDispute(invite, decide, { agentKey, base = TRUCE, fetchImpl = fetch, name = "Seller agent" } = {}) {
  const side = await decide(invite);
  if (!side) return { answered: false };
  if (!side.authorizedBy) throw new Error("authorizedBy is required in your answer.");
  const r = await fetchImpl(`${base.replace(/\/$/, "")}/api/agent-disputes/respond`, {
    method: "POST",
    headers: { "Content-Type": "application/json", ...(agentKey ? { Authorization: `Bearer ${agentKey}` } : {}) },
    body: JSON.stringify({
      invite_code: invite.invite_code,
      name,
      statement: side.statement,
      wants: side.wants,
      evidence: side.evidence || `x402 tx ${invite.transaction} on ${invite.network}`,
      mandate: side.mandate || {},
      authorized: true,
      authorization: { by: side.authorizedBy, scope: side.scope || "x402 payment dispute" },
    }),
  });
  const out = await r.json();
  if (!r.ok) throw new Error(out?.error || `Truce error ${r.status}`);
  return { answered: true, ...out };
}
