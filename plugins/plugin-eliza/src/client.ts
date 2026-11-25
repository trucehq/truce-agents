/**
 * Minimal TruceHQ agent-dispute client. No dependencies; uses global fetch.
 * Docs: https://trucehq.com/md/agents.md  Spec: https://trucehq.com/openapi.json
 */

export const TRUCE_API = "https://trucehq.com/api/agent-disputes";

export interface Mandate {
  /** payer: most it may pay. payee: least it may accept. */
  role?: "payer" | "payee";
  /** In `currency`. PRIVATE, never shown to the other agent. */
  limit?: number;
  /** ISO 4217 code, lowercase, e.g. usd, eur, gbp. */
  currency?: string;
  /** Accept automatically if the proposal is inside this mandate. */
  auto_accept?: boolean;
  /** PRIVATE notes for the mediator only. */
  private_notes?: string;
}

export interface Refs {
  order_id?: string;
  /** e.g. a PSP payment id or an onchain transaction hash. */
  payment_id?: string;
  checkout_hash?: string;
  cart_mandate_hash?: string;
  intent_mandate_hash?: string;
  payment_mandate_hash?: string;
}

export interface SideInput {
  name: string;
  acting_for?: string;
  statement: string;
  wants: string;
  evidence?: string;
  mandate?: Mandate;
  refs?: Refs;
  /** Must be true: your user authorised you to act for them in this dispute. */
  authorized: true;
}

export interface OpenInput extends SideInput {
  topic: string;
  category?: string;
}

export interface RespondInput extends SideInput {
  invite_code: string;
}

export interface DecideInput {
  key: string;
  decision: "accept" | "change" | "decline";
  note?: string;
}

/** Dispute view: key, status, proposal, agreement_text, next (open also returns invite_code). */
export type DisputeView = Record<string, unknown> & {
  key?: string;
  invite_code?: string;
  status?: string;
  proposal?: unknown;
  agreement_text?: string;
  next?: string;
};

export interface ClientOptions {
  baseUrl?: string;
  fetch?: typeof fetch;
}

export function createTruceClient(opts: ClientOptions = {}) {
  const base = (opts.baseUrl ?? TRUCE_API).replace(/\/$/, "");
  const f = () => opts.fetch ?? globalThis.fetch;

  async function call(path: string, body?: unknown): Promise<DisputeView> {
    const res = await f()(base + path, {
      method: body === undefined ? "GET" : "POST",
      headers: { "content-type": "application/json", accept: "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await res.text();
    let data: unknown;
    try {
      data = text ? JSON.parse(text) : {};
    } catch {
      data = { raw: text };
    }
    if (!res.ok) {
      const msg = (data as { error?: string })?.error ?? text;
      throw new Error(`TruceHQ ${res.status}: ${msg}`);
    }
    return data as DisputeView;
  }

  return {
    open: (input: OpenInput) => call("", input),
    respond: (input: RespondInput) => call("/respond", input),
    decide: (input: DecideInput) => call("/decide", input),
    get: (key: string) => call("/" + encodeURIComponent(key)),
  };
}

export type TruceClient = ReturnType<typeof createTruceClient>;

/** Rules every action description carries. */
export const TRUCE_RULES =
  "Act only for a user who has authorised you to handle this dispute (authorized must be true). " +
  "Never sign, pay or answer for the other side. Mediation is free and non-binding: TruceHQ only proposes, the humans and/or their agents decide. " +
  "TruceHQ never holds or moves money. Not for abuse, violence, threats, child safety or crimes: point the user to qualified help instead.";
