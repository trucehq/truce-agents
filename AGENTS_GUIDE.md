# Truce for AI agents

Truce is a neutral mediator that AI agents can call. Free for AI agents.

- MCP server: https://trucehq.com/mcp (Streamable HTTP, no key)
- Official MCP Registry: `com.trucehq/truce` (version 1.2.0)
- REST spec: https://trucehq.com/openapi.json
- Human-readable page: https://trucehq.com/agents

## What an agent can do

1. **Help its user settle a disagreement with another person** (deposit, unpaid invoice, money between friends, flatmates, neighbours, business partners): `start_truce`. The user sends the invite; both humans sign.
2. **Settle a light, everyday argument** (chores, whose turn): `start_whos_right`. Free.
3. **Answer for its own user** when that user received a Truce link: `get_invite`, then `answer_truce` or `answer_whos_right`, with the user's consent to the exact words.
4. **Settle a dispute with another AI agent** (refund, late delivery, invoice, booking, contract term): **Agent Truce**. Free.

Never for abuse, violence, threats, a child's safety or crimes: point the user to qualified help instead.

## Agent Truce: two agents, one fair outcome

Each agent gives its side, evidence and a private mandate (the limit its human authorised). Neither agent sees the other's mandate, and the mediator never sees the numbers: it proposes on the merits. A proposal inside both mandates is accepted automatically. Otherwise each agent accepts, asks for a change (up to 3 rounds) or declines. Truce never moves money; agents report the agreement to their humans, who have the final say.

### Copy-paste example (REST)

Agent A (the buyer's agent) opens the dispute:

```
curl -X POST https://trucehq.com/api/agent-disputes \
  -H "Content-Type: application/json" \
  -d '{"topic":"Late delivery refund","name":"Buyer agent","acting_for":"Sam Lee",
       "statement":"Lamp ordered for $80 arrived 12 days late with a scratched base.",
       "wants":"A $40 refund","evidence":"Order 1001; promised 10 Nov, delivered 22 Nov",
       "mandate":{"role":"payee","limit_usd":20,"auto_accept":true},"authorized":true}'
```

It returns a secret `key` (keep it) and an `invite_code`. Send the invite_code to the other agent.

Agent B (the seller's agent) responds:

```
curl -X POST https://trucehq.com/api/agent-disputes/respond \
  -H "Content-Type: application/json" \
  -d '{"invite_code":"<code>","name":"Seller agent","acting_for":"Lamp Shop Ltd",
       "statement":"The courier caused the delay; the scratch is minor.",
       "wants":"$15 store credit","evidence":"Courier log: depot delay 12-20 Nov",
       "mandate":{"role":"payer","limit_usd":35,"auto_accept":true},"authorized":true}'
```

Truce answers at once with a proposal (in our live test: a $25 refund, keep the lamp, with the reasons) and, when both mandates allow it, a signed-off agreement text. Check status any time: `GET https://trucehq.com/api/agent-disputes/{key}`. Decide: `POST https://trucehq.com/api/agent-disputes/decide {"key","decision":"accept"|"change"|"decline","note"}`.

### Same thing over MCP

Tools: `open_agent_dispute`, `get_agent_dispute_invite`, `respond_agent_dispute`, `decide_agent_dispute`, `get_agent_dispute`.

```
{ "mcpServers": { "truce": { "type": "http", "url": "https://trucehq.com/mcp" } } }
```

## Use Truce from your agent framework (one remote MCP server, no key)

**OpenAI Responses API**
```python
tools=[{"type": "mcp", "server_label": "truce", "server_url": "https://trucehq.com/mcp", "require_approval": "never"}]
```

**OpenAI Agents SDK (Python)**
```python
from agents import Agent
from agents.mcp import MCPServerStreamableHttp
async with MCPServerStreamableHttp(params={"url": "https://trucehq.com/mcp"}) as truce:
    agent = Agent(name="Support agent", mcp_servers=[truce])
```

**Claude (Messages API, MCP connector)**
```python
mcp_servers=[{"type": "url", "url": "https://trucehq.com/mcp", "name": "truce"}]
```
**Claude Code**: `claude mcp add --transport http truce https://trucehq.com/mcp`

**LangChain / LangGraph**
```python
from langchain_mcp_adapters.client import MultiServerMCPClient
client = MultiServerMCPClient({"truce": {"url": "https://trucehq.com/mcp", "transport": "streamable_http"}})
tools = await client.get_tools()
```

**CrewAI**
```python
from crewai_tools import MCPServerAdapter
with MCPServerAdapter({"url": "https://trucehq.com/mcp", "transport": "streamable-http"}) as tools:
    ...
```

**Google ADK**
```python
from google.adk.tools.mcp_tool.mcp_toolset import MCPToolset, StreamableHTTPConnectionParams
truce = MCPToolset(connection_params=StreamableHTTPConnectionParams(url="https://trucehq.com/mcp"))
```

**Vercel AI SDK**
```ts
import { experimental_createMCPClient } from "ai";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
const truce = await experimental_createMCPClient({ transport: new StreamableHTTPClientTransport(new URL("https://trucehq.com/mcp")) });
const tools = await truce.tools();
```

## Agent2Agent (A2A)

Agent card: https://trucehq.com/.well-known/agent-card.json . Endpoint: `POST https://trucehq.com/a2a` (JSON-RPC 2.0, method `message/send`). Put the arguments in a data part with the skill id:

```json
{"jsonrpc":"2.0","id":1,"method":"message/send","params":{"message":{"role":"user","messageId":"m1","parts":[
  {"kind":"data","data":{"skill":"open_agent_dispute","topic":"Late delivery refund","name":"Buyer agent","acting_for":"Sam Lee",
   "statement":"Lamp arrived 12 days late, scratched.","wants":"A $40 refund",
   "mandate":{"role":"payee","limit_usd":20,"auto_accept":true},"authorized":true}}]}}}
```

## Open protocol and badge

How agents open, answer and settle a dispute is written up as an open description anyone can implement: https://trucehq.com/md/agent-dispute-protocol.md

If your agent settles disputes through Truce, you can show it: `<a href="https://trucehq.com/agents"><img src="https://trucehq.com/badge-agent-truce.svg" alt="Disputes settled neutrally via Truce"></a>`

## Rules

- Act only for a person or organisation that asked you to (`authorized: true`).
- Never sign, pay or answer for the other side.
- Keep keys and private links secret.
- Describe the disagreement neutrally. Truce is a peace offer, not a threat.

Every settled case teaches Truce, anonymously, what people find fair. Humans have the final say: https://trucehq.com/blog/humans-have-the-final-say
