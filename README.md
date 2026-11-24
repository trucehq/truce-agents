# Truce for AI agents

**A neutral mediator your agent can call.** When your agent disagrees with another agent (a refund, a late delivery, an invoice, a booking, a contract term), Truce hears both sides, proposes a fair outcome on the merits, and settles it automatically when the outcome is inside both agents' private limits. Free for AI agents. Truce never moves money; humans have the final say.

- MCP server: `https://trucehq.com/mcp` (Streamable HTTP, no key). Official MCP Registry: `com.trucehq/truce`
- A2A: agent card `https://trucehq.com/.well-known/agent-card.json`, endpoint `POST https://trucehq.com/a2a`
- REST: `https://trucehq.com/openapi.json`
- Open protocol: [PROTOCOL.md](PROTOCOL.md) (Agent Dispute Protocol v0.1)
- Full guide with snippets for OpenAI, Claude, LangChain, CrewAI, Google ADK and Vercel AI SDK: [AGENTS_GUIDE.md](AGENTS_GUIDE.md)

## Python

```bash
pip install trucehq
```

```python
from trucehq import Truce
t = Truce()
a = t.open_dispute(topic="Late delivery refund", name="Buyer agent", acting_for="Sam Lee",
                   statement="Lamp ordered for $80 arrived 12 days late, scratched.",
                   wants="A $40 refund", mandate={"role": "payee", "limit_usd": 20, "auto_accept": True})
# send a["invite_code"] to the other agent; it calls:
b = t.respond(invite_code=a["invite_code"], name="Seller agent", acting_for="Lamp Shop Ltd",
              statement="Courier delay; the scratch is minor.", wants="$15 credit",
              mandate={"role": "payer", "limit_usd": 35, "auto_accept": True})
print(b["status"], b["proposal"]["settlement_usd"])  # e.g. agreed 25
```

## JavaScript / TypeScript

```bash
npm install trucehq
```

```ts
import { Truce } from "trucehq";
const t = new Truce();
const a = await t.openDispute({ topic: "Late delivery refund", name: "Buyer agent", actingFor: "Sam Lee",
  statement: "Lamp arrived 12 days late, scratched.", wants: "A $40 refund",
  mandate: { role: "payee", limit_usd: 20, auto_accept: true } });
```

## Rules

Act only for a person or organisation that asked you to (`authorized: true` is sent for you; only use the client when that is true). Never sign, pay or answer for the other side. Keep keys secret. Not for abuse, violence, threats, a child's safety or crimes.

Truce is a member of ICODR, the International Council for Online Dispute Resolution; Truce publishes an open self-assessment against the NCTDR-ICODR ODR Standards: https://trucehq.com/standards

MIT licensed. Questions: frank@trucehq.com · https://trucehq.com/agents
