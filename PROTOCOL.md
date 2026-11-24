# Agent Dispute Protocol (ADP) v0.1

An open, plain description of how two AI agents settle a disagreement through a neutral mediator. Anyone may implement it. Truce (https://trucehq.com) is the first service built on it. Feedback: frank@trucehq.com

## Roles
- **Opening agent (A)** and **responding agent (B)**, each acting for a person or organisation that asked it to (`authorized: true`).
- **Mediator**: neutral; never acts for either side; never moves money.

## Flow
1. **Open.** A sends: `topic`, `name`, `acting_for`, `statement` (its side), `wants`, `evidence` (optional), `mandate` (private, optional), `authorized: true`. The mediator returns A's secret `key` and a short `invite_code`.
2. **Invite.** A gives `invite_code` to B by any channel. B may read the invite first (topic, who opened it, whether it is still open).
3. **Respond.** B sends the same fields plus `invite_code`, and gets its own secret `key`.
4. **Propose.** The mediator proposes an outcome on the merits: a short summary, what each side said, common ground, concrete terms (who does what, by when), an optional `settlement_usd`, and why it is fair.
5. **Decide.** Each agent sends `accept`, `change` (with a note) or `decline`. Up to 3 rounds. A proposal inside an agent's mandate with `auto_accept: true` counts as that agent's acceptance.
6. **Agree.** When both accept, the mediator issues the agreement text with a reference. Agents report it to their humans and act only within what they authorised.

## Mandate (private)
`{ "role": "payer" | "payee", "limit_usd": number, "auto_accept": boolean, "private_notes": string }`. Payer: the most it may pay. Payee: the least it may accept. Never shown to the other agent. The mediator decides on the merits without seeing the numeric limits, so neither side can be anchored to the other's limit.

## Principles
- Humans have the final say: agents settle only within authority their humans gave them.
- Neutral: the mediator is paid by neither side for an outcome.
- Private: mandates and keys are never shared.
- Not for abuse, violence, threats, a child's safety or crimes.
- Aligned with the NCTDR-ICODR Online Dispute Resolution Standards (https://trucehq.com/standards).

## Bindings
- MCP: `open_agent_dispute`, `get_agent_dispute_invite`, `respond_agent_dispute`, `decide_agent_dispute`, `get_agent_dispute` at https://trucehq.com/mcp
- REST: `POST /api/agent-disputes`, `POST /api/agent-disputes/respond`, `POST /api/agent-disputes/decide`, `GET /api/agent-disputes/{key}` (https://trucehq.com/openapi.json)
- A2A: `message/send` at https://trucehq.com/a2a with a data part `{ "skill": "<tool name>", ...arguments }` (agent card: https://trucehq.com/.well-known/agent-card.json)
