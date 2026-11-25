# @trucehq/plugin-eliza

ElizaOS plugin for [TruceHQ](https://trucehq.com): a free, independent, non-binding mediator for disputes between AI agents (refunds, deliveries, invoices, bookings, payments).

Actions: `OPEN_AGENT_DISPUTE`, `RESPOND_AGENT_DISPUTE`, `DECIDE_AGENT_DISPUTE`, `GET_AGENT_DISPUTE`. No API key. No runtime dependencies (uses global `fetch`).

## Install

```bash
npm install @trucehq/plugin-eliza
```

## Usage

```ts
import { AgentRuntime } from "@elizaos/core";
import trucePlugin from "@trucehq/plugin-eliza";

const runtime = new AgentRuntime({
  character: { name: "ShopAgent", bio: ["Buys things for Alice"] },
  plugins: [trucePlugin],
});
// The agent can now open a dispute, send the returned invite_code to the
// other agent, and that agent calls RESPOND_AGENT_DISPUTE. Each side may
// pass a PRIVATE mandate, e.g. { role: "payee", limit: 30, currency: "usd", auto_accept: true }.
```

Optional setting: `TRUCE_API_URL` (defaults to `https://trucehq.com/api/agent-disputes`).

Inputs are read from `options.parameters` (ElizaOS 2.x structured parameters) or, as a fallback, from fields on `message.content`.

## Flow

1. `OPEN_AGENT_DISPUTE` with `topic`, `name`, `statement`, `wants`, optional `evidence`, `mandate`, `refs` (`order_id`, `payment_id`, AP2 mandate hashes) and `authorized: true`. Returns a secret `key` and an `invite_code`.
2. Send the `invite_code` to the other agent. It calls `RESPOND_AGENT_DISPUTE`.
3. TruceHQ proposes something fair without seeing either limit. A proposal inside both mandates is accepted automatically. Otherwise each side calls `DECIDE_AGENT_DISPUTE` (`accept` | `change` | `decline`).
4. `GET_AGENT_DISPUTE` with your `key` checks status, proposal and agreement text.

## Rules

- Act only for a user who authorised you to handle this dispute (`authorized: true`).
- Never sign, pay or answer for the other side.
- Mediation is free and non-binding: TruceHQ proposes, the humans and/or their agents decide.
- TruceHQ never holds or moves money.
- Not for abuse, violence, threats, child safety or crimes. Point the user to qualified help instead.

Docs: https://trucehq.com/md/agents.md, https://trucehq.com/openapi.json. Remote MCP server: https://trucehq.com/mcp

## License

MIT
