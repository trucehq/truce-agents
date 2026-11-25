import { z } from "zod";
import { ActionProvider, CreateAction, type Network } from "@coinbase/agentkit";
import { createTruceClient, TRUCE_RULES, type ClientOptions, type TruceClient } from "./client";

const Mandate = z
  .object({
    role: z.enum(["payer", "payee"]).optional().describe("payer: most it may pay. payee: least it may accept."),
    limit: z.number().optional().describe("Limit in `currency`. PRIVATE, never shown to the other agent."),
    currency: z.string().optional().describe("ISO 4217 code, lowercase, e.g. usd, eur, gbp."),
    auto_accept: z.boolean().optional().describe("Accept automatically if the proposal is inside this mandate."),
    private_notes: z.string().optional().describe("PRIVATE notes for the mediator only."),
  })
  .describe("PRIVATE mandate your user authorised; never shown to the other agent.");

const Refs = z
  .object({
    order_id: z.string().optional(),
    payment_id: z.string().optional().describe("Payment id, e.g. an onchain transaction hash (0x...)."),
    checkout_hash: z.string().optional(),
    cart_mandate_hash: z.string().optional(),
    intent_mandate_hash: z.string().optional(),
    payment_mandate_hash: z.string().optional(),
  })
  .describe("Optional references to the order or payment (AP2 mandate hashes supported).");

const Side = {
  name: z.string().describe("Name of your user (the side you act for)."),
  acting_for: z.string().optional().describe("Who you act for, e.g. 'Acme Ltd (buyer)'."),
  statement: z.string().describe("Your side of the dispute, factual and calm."),
  wants: z.string().describe("What your side wants as an outcome."),
  evidence: z.string().optional().describe("Evidence: order ids, tx hashes, logs, messages."),
  mandate: Mandate.optional(),
  refs: Refs.optional(),
  authorized: z.literal(true).describe("Must be true: your user authorised you to act for them in this dispute."),
};

export const OpenAgentDisputeSchema = z.object({
  topic: z.string().describe("Short title of the dispute."),
  category: z.string().optional().describe("Optional category, e.g. refund, delivery, invoice."),
  ...Side,
});

export const RespondAgentDisputeSchema = z.object({
  invite_code: z.string().describe("Invite code received from the other agent."),
  ...Side,
});

export const DecideAgentDisputeSchema = z.object({
  key: z.string().describe("Your secret dispute key."),
  decision: z.enum(["accept", "change", "decline"]),
  note: z.string().optional(),
});

export const GetAgentDisputeSchema = z.object({
  key: z.string().describe("Your secret dispute key."),
});

const asText = async (p: Promise<unknown>) => {
  try {
    return JSON.stringify(await p);
  } catch (e) {
    return `Error: ${(e as Error).message}`;
  }
};

/**
 * TruceActionProvider lets an AgentKit agent use TruceHQ, a free, independent,
 * non-binding mediator for disputes between AI agents. Network-agnostic: it
 * never signs or sends transactions and TruceHQ never holds or moves money.
 */
export class TruceActionProvider extends ActionProvider {
  private readonly client: TruceClient;

  constructor(opts: ClientOptions = {}) {
    super("truce", []);
    this.client = createTruceClient(opts);
  }

  @CreateAction({
    name: "open_agent_dispute",
    description:
      "Open a TruceHQ mediation when you disagree with another AI agent about a refund, delivery, invoice, booking or payment " +
      "(put an onchain tx hash in refs.payment_id). Returns a secret key (keep it) and an invite_code to send to the other agent. " +
      TRUCE_RULES,
    schema: OpenAgentDisputeSchema,
  })
  async openAgentDispute(args: z.infer<typeof OpenAgentDisputeSchema>): Promise<string> {
    return asText(this.client.open(args));
  }

  @CreateAction({
    name: "respond_agent_dispute",
    description:
      "Respond to a TruceHQ agent dispute with the invite_code another agent sent you; gives your side and returns key, status and any proposal. " +
      TRUCE_RULES,
    schema: RespondAgentDisputeSchema,
  })
  async respondAgentDispute(args: z.infer<typeof RespondAgentDisputeSchema>): Promise<string> {
    return asText(this.client.respond(args));
  }

  @CreateAction({
    name: "decide_agent_dispute",
    description:
      "Accept, ask to change, or decline a TruceHQ proposal for your own side only, using your secret key. Only accept within what your user authorised. " +
      TRUCE_RULES,
    schema: DecideAgentDisputeSchema,
  })
  async decideAgentDispute(args: z.infer<typeof DecideAgentDisputeSchema>): Promise<string> {
    return asText(this.client.decide(args));
  }

  @CreateAction({
    name: "get_agent_dispute",
    description: "Get status, proposal and agreement text of a TruceHQ agent dispute using your secret key. " + TRUCE_RULES,
    schema: GetAgentDisputeSchema,
  })
  async getAgentDispute(args: z.infer<typeof GetAgentDisputeSchema>): Promise<string> {
    return asText(this.client.get(args.key));
  }

  /** Off-chain service: works on every network. */
  supportsNetwork = (_network: Network) => true;
}

export const truceActionProvider = (opts?: ClientOptions) => new TruceActionProvider(opts);
