import type {
  Action,
  ActionParameter,
  ActionResult,
  HandlerCallback,
  IAgentRuntime,
  Memory,
  Plugin,
} from "@elizaos/core";
import { createTruceClient, TRUCE_RULES, type TruceClient } from "./client.js";

export * from "./client.js";

type Params = Record<string, unknown>;

function p(name: string, description: string, schema: ActionParameter["schema"], required = false): ActionParameter {
  return { name, description, required, schema };
}

const mandateSchema: ActionParameter["schema"] = {
  type: "object",
  description: "PRIVATE mandate your user authorised, never shown to the other agent.",
  properties: {
    role: { type: "string", enum: ["payer", "payee"] },
    limit: { type: "number", description: "Payer: most it may pay. Payee: least it may accept. In currency." },
    currency: { type: "string", description: "ISO 4217 code, lowercase, e.g. usd, eur, gbp." },
    auto_accept: { type: "boolean", description: "Accept automatically if the proposal is inside the mandate." },
    private_notes: { type: "string" },
  },
};

const refsSchema: ActionParameter["schema"] = {
  type: "object",
  description: "Optional references: order_id, payment_id (e.g. onchain tx hash), checkout_hash, cart/intent/payment mandate hashes (AP2).",
  properties: {
    order_id: { type: "string" },
    payment_id: { type: "string" },
    checkout_hash: { type: "string" },
    cart_mandate_hash: { type: "string" },
    intent_mandate_hash: { type: "string" },
    payment_mandate_hash: { type: "string" },
  },
};

const sideParams: ActionParameter[] = [
  p("name", "Name of your user (the side you act for).", { type: "string" }, true),
  p("acting_for", "Who you act for, e.g. 'Acme Ltd (buyer)'.", { type: "string" }),
  p("statement", "Your side of the dispute, factual and calm.", { type: "string" }, true),
  p("wants", "What your side wants as an outcome.", { type: "string" }, true),
  p("evidence", "Evidence: order ids, tx hashes, logs, messages.", { type: "string" }),
  p("mandate", "PRIVATE mandate (role, limit, currency, auto_accept, private_notes).", mandateSchema),
  p("refs", "Optional references (order_id, payment_id, AP2 mandate hashes).", refsSchema),
  p("authorized", "Must be true: your user authorised you to act for them in this dispute.", { type: "boolean" }, true),
];

function readParams(message: Memory, options: unknown): Params {
  const fromOptions = (options as { parameters?: Params } | undefined)?.parameters;
  if (fromOptions && typeof fromOptions === "object") return fromOptions;
  // Fallback for runtimes without structured parameters: fields on message.content.
  return { ...(message.content as Params) };
}

function pick(src: Params, keys: string[]): Params {
  const out: Params = {};
  for (const k of keys) if (src[k] !== undefined && src[k] !== null && src[k] !== "") out[k] = src[k];
  return out;
}

function missing(src: Params, keys: string[]): string[] {
  return keys.filter((k) => src[k] === undefined || src[k] === null || src[k] === "");
}

function clientFor(runtime: IAgentRuntime, override?: TruceClient): TruceClient {
  if (override) return override;
  const base = runtime.getSetting?.("TRUCE_API_URL");
  return createTruceClient(typeof base === "string" && base ? { baseUrl: base } : {});
}

async function finish(
  text: string,
  data: Record<string, unknown>,
  success: boolean,
  callback?: HandlerCallback,
  action?: string,
): Promise<ActionResult> {
  if (callback) await callback({ text, actions: action ? [action] : undefined }, action);
  return { success, text, data: data as ActionResult["data"] };
}

const SIDE_KEYS = ["name", "acting_for", "statement", "wants", "evidence", "mandate", "refs", "authorized"];

export function createTruceActions(client?: TruceClient): Action[] {
  const openAction: Action = {
    name: "OPEN_AGENT_DISPUTE",
    similes: ["START_AGENT_DISPUTE", "TRUCE_OPEN", "MEDIATE_AGENT_DISPUTE"],
    description:
      "Open a TruceHQ mediation when you (an AI agent) disagree with another AI agent about a refund, delivery, invoice, booking or payment. " +
      "Returns a secret key (keep it) and an invite_code: send the invite_code to the other agent so it can call RESPOND_AGENT_DISPUTE. " +
      TRUCE_RULES,
    parameters: [
      p("topic", "Short title of the dispute.", { type: "string" }, true),
      p("category", "Optional category, e.g. refund, delivery, invoice.", { type: "string" }),
      ...sideParams,
    ],
    validate: async () => true,
    handler: async (runtime, message, _state, options, callback) => {
      const params = readParams(message, options);
      const miss = missing(params, ["topic", "name", "statement", "wants"]);
      if (params.authorized !== true) miss.push("authorized=true");
      if (miss.length)
        return finish(`Cannot open dispute, missing: ${miss.join(", ")}.`, { missing: miss }, false, callback, "OPEN_AGENT_DISPUTE");
      try {
        const body = pick(params, ["topic", "category", ...SIDE_KEYS]);
        const r = await clientFor(runtime, client).open(body as never);
        return finish(
          `TruceHQ dispute opened. Send this invite_code to the other agent: ${r.invite_code}. Keep the key secret.`,
          r,
          true,
          callback,
          "OPEN_AGENT_DISPUTE",
        );
      } catch (e) {
        return finish(`TruceHQ error: ${(e as Error).message}`, {}, false, callback, "OPEN_AGENT_DISPUTE");
      }
    },
    examples: [
      [
        { name: "{{user1}}", content: { text: "The seller agent won't refund the $40 for the order that never arrived. You're authorised to settle up to $40 via Truce." } },
        { name: "{{agent}}", content: { text: "Opening a TruceHQ mediation and sending the invite code to the seller agent.", actions: ["OPEN_AGENT_DISPUTE"] } },
      ],
    ],
  };

  const respondAction: Action = {
    name: "RESPOND_AGENT_DISPUTE",
    similes: ["ANSWER_AGENT_DISPUTE", "TRUCE_RESPOND", "JOIN_AGENT_DISPUTE"],
    description:
      "Respond to a TruceHQ agent dispute using the invite_code another agent sent you; gives your side and returns a key, status and possibly a proposal. " +
      TRUCE_RULES,
    parameters: [p("invite_code", "Invite code received from the other agent.", { type: "string" }, true), ...sideParams],
    validate: async () => true,
    handler: async (runtime, message, _state, options, callback) => {
      const params = readParams(message, options);
      const miss = missing(params, ["invite_code", "name", "statement", "wants"]);
      if (params.authorized !== true) miss.push("authorized=true");
      if (miss.length)
        return finish(`Cannot respond, missing: ${miss.join(", ")}.`, { missing: miss }, false, callback, "RESPOND_AGENT_DISPUTE");
      try {
        const body = pick(params, ["invite_code", ...SIDE_KEYS]);
        const r = await clientFor(runtime, client).respond(body as never);
        return finish(`TruceHQ status: ${r.status ?? "unknown"}.${r.proposal ? ` Proposal: ${JSON.stringify(r.proposal)}` : ""}`, r, true, callback, "RESPOND_AGENT_DISPUTE");
      } catch (e) {
        return finish(`TruceHQ error: ${(e as Error).message}`, {}, false, callback, "RESPOND_AGENT_DISPUTE");
      }
    },
    examples: [
      [
        { name: "{{user1}}", content: { text: "The buyer agent sent Truce invite code AB12CD. Answer it for us; we can refund at most $25." } },
        { name: "{{agent}}", content: { text: "Responding to the TruceHQ dispute with our side.", actions: ["RESPOND_AGENT_DISPUTE"] } },
      ],
    ],
  };

  const decideAction: Action = {
    name: "DECIDE_AGENT_DISPUTE",
    similes: ["ACCEPT_TRUCE_PROPOSAL", "DECLINE_TRUCE_PROPOSAL", "TRUCE_DECIDE"],
    description:
      "Accept, ask to change, or decline a TruceHQ proposal for your own side only, using your secret key. Only accept within what your user authorised. " +
      TRUCE_RULES,
    parameters: [
      p("key", "Your secret dispute key.", { type: "string" }, true),
      p("decision", "accept, change or decline.", { type: "string", enum: ["accept", "change", "decline"] }, true),
      p("note", "Optional note, e.g. what to change.", { type: "string" }),
    ],
    validate: async () => true,
    handler: async (runtime, message, _state, options, callback) => {
      const params = readParams(message, options);
      const ok = ["accept", "change", "decline"].includes(String(params.decision));
      if (!params.key || !ok)
        return finish("Cannot decide: need key and decision (accept|change|decline).", {}, false, callback, "DECIDE_AGENT_DISPUTE");
      try {
        const r = await clientFor(runtime, client).decide(pick(params, ["key", "decision", "note"]) as never);
        return finish(`TruceHQ decision recorded. Status: ${r.status ?? "unknown"}.`, r, true, callback, "DECIDE_AGENT_DISPUTE");
      } catch (e) {
        return finish(`TruceHQ error: ${(e as Error).message}`, {}, false, callback, "DECIDE_AGENT_DISPUTE");
      }
    },
    examples: [
      [
        { name: "{{user1}}", content: { text: "That Truce proposal is fine, accept it." } },
        { name: "{{agent}}", content: { text: "Accepting the TruceHQ proposal for our side.", actions: ["DECIDE_AGENT_DISPUTE"] } },
      ],
    ],
  };

  const getAction: Action = {
    name: "GET_AGENT_DISPUTE",
    similes: ["CHECK_AGENT_DISPUTE", "TRUCE_STATUS"],
    description: "Get the status, proposal and agreement text of a TruceHQ agent dispute using your secret key. " + TRUCE_RULES,
    parameters: [p("key", "Your secret dispute key.", { type: "string" }, true)],
    validate: async () => true,
    handler: async (runtime, message, _state, options, callback) => {
      const params = readParams(message, options);
      if (!params.key) return finish("Cannot check status: need key.", {}, false, callback, "GET_AGENT_DISPUTE");
      try {
        const r = await clientFor(runtime, client).get(String(params.key));
        return finish(`TruceHQ status: ${r.status ?? "unknown"}.${r.next ? ` Next: ${r.next}` : ""}`, r, true, callback, "GET_AGENT_DISPUTE");
      } catch (e) {
        return finish(`TruceHQ error: ${(e as Error).message}`, {}, false, callback, "GET_AGENT_DISPUTE");
      }
    },
    examples: [
      [
        { name: "{{user1}}", content: { text: "Any update on the Truce dispute?" } },
        { name: "{{agent}}", content: { text: "Checking the TruceHQ dispute status.", actions: ["GET_AGENT_DISPUTE"] } },
      ],
    ],
  };

  return [openAction, respondAction, decideAction, getAction];
}

export const trucePlugin: Plugin = {
  name: "trucehq",
  description:
    "TruceHQ: free, independent, non-binding mediation for disputes between AI agents. Never holds or moves money.",
  actions: createTruceActions(),
};

export default trucePlugin;
