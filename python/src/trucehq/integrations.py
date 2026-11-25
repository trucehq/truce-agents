"""Truce tools for agent frameworks: LangChain, CrewAI and the OpenAI Agents SDK.

Each helper returns the same four tools, bound to one Truce client and to the person who
authorised your agent to handle disputes (`authorized_by`). The framework packages are
imported only when you call their helper, so `trucehq` itself has no dependencies.

    from trucehq import Truce
    from trucehq.integrations import langchain_tools
    tools = langchain_tools(Truce(), authorized_by="Sam Lee, account owner")
"""
from __future__ import annotations

import json
from typing import Any, Callable, Dict, List, Optional

from . import Truce

GUIDE = (
    "Truce is a neutral, non-binding mediator for disputes with another AI agent (a refund, a "
    "delivery, an invoice, a paid API call). Only use it for a disagreement your human asked you "
    "to handle. Never share your private mandate with the other side."
)


def _funcs(truce: Truce, authorized_by: str, scope: str) -> Dict[str, Callable[..., str]]:
    if not authorized_by or not str(authorized_by).strip():
        raise ValueError("authorized_by is required: the person or role who asked your agent to act.")

    def dump(x: Any) -> str:
        return json.dumps(x, ensure_ascii=False)

    def truce_open_dispute(topic: str, statement: str, wants: str, evidence: str = "",
                           acting_for: str = "", max_refund_usd: Optional[float] = None) -> str:
        """Open a Truce with another agent. Returns invite_code (send it to the other agent) and
        your private key (keep it). max_refund_usd is your private limit, never shown to them."""
        mandate = {} if max_refund_usd is None else {"role": "payer", "limit": max_refund_usd, "currency": "usd"}
        return dump(truce.open_dispute(topic, "Agent", statement, wants, authorized_by=authorized_by,
                                       scope=scope, acting_for=acting_for, evidence=evidence, mandate=mandate))

    def truce_respond(invite_code: str, statement: str, wants: str, evidence: str = "",
                      acting_for: str = "", min_accept_usd: Optional[float] = None) -> str:
        """Answer a Truce another agent opened with you, using its invite_code. min_accept_usd is
        your private limit, never shown to them."""
        mandate = {} if min_accept_usd is None else {"role": "payee", "limit": min_accept_usd, "currency": "usd"}
        return dump(truce.respond(invite_code, "Agent", statement, wants, authorized_by=authorized_by,
                                  scope=scope, acting_for=acting_for, evidence=evidence, mandate=mandate))

    def truce_status(key: str) -> str:
        """Check a Truce: the proposal, what each side decided, and the agreement once both accept."""
        return dump(truce.get(key))

    def truce_decide(key: str, decision: str, note: str = "") -> str:
        """Answer Truce's proposal: decision is 'accept', 'change' (with a note) or 'decline'.
        Only accept what your human authorised."""
        return dump(truce.decide(key, decision, note))

    return {f.__name__: f for f in (truce_open_dispute, truce_respond, truce_status, truce_decide)}


def tool_specs() -> List[dict]:
    """JSON-schema function definitions (OpenAI function calling and similar)."""
    s, n = {"type": "string"}, {"type": "number"}
    return [
        {"name": "truce_open_dispute", "description": "Open a neutral Truce with another AI agent. " + GUIDE,
         "parameters": {"type": "object", "required": ["topic", "statement", "wants"], "properties": {
             "topic": s, "statement": s, "wants": s, "evidence": s, "acting_for": s, "max_refund_usd": n}}},
        {"name": "truce_respond", "description": "Answer a Truce another agent opened with you. " + GUIDE,
         "parameters": {"type": "object", "required": ["invite_code", "statement", "wants"], "properties": {
             "invite_code": s, "statement": s, "wants": s, "evidence": s, "acting_for": s, "min_accept_usd": n}}},
        {"name": "truce_status", "description": "Check a Truce and its proposal.",
         "parameters": {"type": "object", "required": ["key"], "properties": {"key": s}}},
        {"name": "truce_decide", "description": "Accept, change or decline Truce's proposal.",
         "parameters": {"type": "object", "required": ["key", "decision"], "properties": {
             "key": s, "decision": {"type": "string", "enum": ["accept", "change", "decline"]}, "note": s}}},
    ]


def call(name: str, arguments: dict, truce: Truce, authorized_by: str, scope: str = "agent disputes") -> str:
    """Run one tool by name, e.g. from an OpenAI tool call: call(tc.name, json.loads(tc.arguments), ...)."""
    return _funcs(truce, authorized_by, scope)[name](**arguments)


def langchain_tools(truce: Optional[Truce] = None, *, authorized_by: str, scope: str = "agent disputes") -> list:
    """LangChain StructuredTools. Needs `pip install langchain-core`."""
    from langchain_core.tools import StructuredTool

    return [StructuredTool.from_function(f, name=name, description=(f.__doc__ or "") + " " + GUIDE)
            for name, f in _funcs(truce or Truce(), authorized_by, scope).items()]


def openai_agents_tools(truce: Optional[Truce] = None, *, authorized_by: str, scope: str = "agent disputes") -> list:
    """OpenAI Agents SDK function tools. Needs `pip install openai-agents`."""
    from agents import function_tool

    return [function_tool(f, name_override=name, description_override=(f.__doc__ or "") + " " + GUIDE)
            for name, f in _funcs(truce or Truce(), authorized_by, scope).items()]


def crewai_tools(truce: Optional[Truce] = None, *, authorized_by: str, scope: str = "agent disputes") -> list:
    """CrewAI tools. Needs `pip install crewai`."""
    from crewai.tools import tool

    out = []
    for name, f in _funcs(truce or Truce(), authorized_by, scope).items():
        f.__doc__ = (f.__doc__ or "") + " " + GUIDE
        out.append(tool(name)(f))
    return out
