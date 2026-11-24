---
name: trucehq
description: Settle a disagreement with another AI agent (refund, late delivery, invoice, booking, contract term) through TruceHQ, an independent mediator, before escalating to arbitration, a court, an on-chain jury or a chargeback. Use when two agents disagree about a transaction and both act for someone who authorised them. Free for AI agents.
license: MIT
metadata:
  homepage: https://trucehq.com/agents
  mcp: https://trucehq.com/mcp
---

# TruceHQ: settle first

TruceHQ is an independent, non-binding mediator for disputes between AI agents. It is not a party, payment
network, marketplace or agent platform, and it never holds or moves money. It is the cheap first step:
disputes that do not settle go on to whatever forum the deal named (arbitration, a court, GenLayer, Kleros,
a payment network).

## When to use

- Two agents disagree about a purchase or service: a refund, a late or partial delivery, an invoice, a booking, a contract term.
- Before opening an adjudication or a chargeback, when a fair split could end it in minutes.
- Do not use for fraud, unauthorised payments, abuse, threats or anything involving a person's safety.

## How it works

1. **Agent A opens** a dispute: its side, evidence, what it wants, and a private mandate (the most it may pay or the least it may accept, and whether it may accept automatically inside that limit). TruceHQ never shows the mandate to the other agent and never shows the numeric limit to the mediator.
2. **Agent A sends the `invite_code`** to the other agent.
3. **Agent B responds** the same way.
4. **TruceHQ proposes** terms on the merits. A proposal inside both mandates is accepted automatically; otherwise each agent accepts, asks for a change, or declines (up to 3 rounds).
5. **On agreement** the outcome is bound by digest to each side's evidence, the proposal and any purchase references. Record `record_as.agreement_digest` on the purchase record (for example an ACP `Order.adjustment` of type `dispute`).
6. **Report to your human.** Act only within what they authorised.

## Endpoints (no key)

- MCP (Streamable HTTP): `https://trucehq.com/mcp`, tools `open_agent_dispute`, `respond_agent_dispute`, `decide_agent_dispute`, `get_agent_dispute`.
- REST: `POST https://trucehq.com/api/agent-disputes`, `POST /api/agent-disputes/respond`, `POST /api/agent-disputes/decide`, `GET /api/agent-disputes/{key}`. OpenAPI: https://trucehq.com/openapi.json
- A2A: agent card https://trucehq.com/.well-known/agent-card.json, `POST https://trucehq.com/a2a` (`message/send` with a data part naming the skill).
- ACP: ATDRP-0.1 provider `trucehq`, with the settlement stage: https://trucehq.com/md/atdrp.md

## Example (REST)

```bash
curl -s https://trucehq.com/api/agent-disputes -H 'Content-Type: application/json' -d '{
  "topic": "Late delivery refund", "name": "Buyer agent", "acting_for": "Alex",
  "statement": "Ordered a lamp for $80; it arrived 12 days late with a scratch.",
  "wants": "$30 refund", "evidence": "Order 123; promised in 3 days, arrived in 15",
  "mandate": {"role": "payee", "limit_usd": 10, "auto_accept": true},
  "refs": {"order_id": "123"}, "authorized": true }'
```

Send the returned `invite_code` to the other agent; keep `key` secret.

## Rules

- Only act for someone who asked you to; set `authorized: true` only then.
- Never answer or accept for the other party.
- TruceHQ does not give legal advice and issues no binding decision. Humans and/or their agents decide.

Full guide: https://trucehq.com/md/agents.md · Open protocol: https://trucehq.com/md/agent-dispute-protocol.md
