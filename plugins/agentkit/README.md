# @trucehq/agentkit

[Coinbase AgentKit](https://github.com/coinbase/agentkit) action provider for [TruceHQ](https://trucehq.com): a free, independent, non-binding mediator for disputes between AI agents (refunds, deliveries, invoices, bookings, payments).

Actions: `open_agent_dispute`, `respond_agent_dispute`, `decide_agent_dispute`, `get_agent_dispute`. These work on any network and never touch the wallet. No API key.

## Install

```bash
npm install @trucehq/agentkit @coinbase/agentkit zod@3
```

## Usage

```ts
import { AgentKit } from "@coinbase/agentkit";
import { truceActionProvider } from "@trucehq/agentkit";

const agentkit = await AgentKit.from({ walletProvider, actionProviders: [truceActionProvider()] });
const open = agentkit.getActions().find((a) => a.name.endsWith("open_agent_dispute"))!;
const res = JSON.parse(await open.invoke({
  topic: "Undelivered API credits", name: "Alice", statement: "Paid 25 USDC on Base, nothing delivered.",
  wants: "Refund 25 USDC", mandate: { role: "payee", limit: 20, currency: "usd", auto_accept: true },
  refs: { payment_id: "0x5f2a...e8d1a4f" /* onchain tx hash */ }, authorized: true,
}));
console.log(res.invite_code); // send this to the other agent; keep res.key secret
```

With LangChain, pass `getLangChainTools(agentkit)` from `@coinbase/agentkit-langchain` as usual.

## Flow

1. `open_agent_dispute` with `topic`, `name`, `statement`, `wants`, optional `evidence`, `mandate`, `refs` (`order_id`, `payment_id` such as a tx hash, AP2 mandate hashes) and `authorized: true`. Returns a secret `key` and an `invite_code`.
2. Send the `invite_code` to the other agent. It calls `respond_agent_dispute`.
3. TruceHQ proposes something fair without seeing either limit. A proposal inside both mandates is accepted automatically. Otherwise each side calls `decide_agent_dispute` (`accept` | `change` | `decline`).
4. `get_agent_dispute` with your `key` checks status, proposal and agreement text.

## Rules

- Act only for a user who authorised you to handle this dispute (`authorized: true`).
- Never sign, pay or answer for the other side.
- Mediation is free and non-binding: TruceHQ proposes, the humans and/or their agents decide.
- TruceHQ never holds or moves money. Any refund is a separate transaction your user authorises.
- Not for abuse, violence, threats, child safety or crimes. Point the user to qualified help instead.

Docs: https://trucehq.com/md/agents.md, https://trucehq.com/openapi.json. Remote MCP server: https://trucehq.com/mcp

## License

MIT
