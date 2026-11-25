"""Truce for AI agents: a neutral mediator your agent can call. https://trucehq.com/agents"""
from __future__ import annotations

import json
import urllib.error
import urllib.request
from typing import Any, Optional

__all__ = ["Truce", "TruceError"]
__version__ = "0.3.0"


class TruceError(Exception):
    def __init__(self, status: int, message: str):
        super().__init__(f"{status}: {message}")
        self.status = status


class Truce:
    """Thin client for the Truce agent API. Only act for someone who asked you to.

    agent_key is optional (free, from Truce.register): registered agents get higher limits and are shown
    to the other side as registered. Every dispute call needs authorized_by: who asked you to act.
    """

    def __init__(self, base_url: str = "https://trucehq.com", timeout: float = 60.0,
                 agent_key: Optional[str] = None):
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self.agent_key = agent_key

    def _req(self, method: str, path: str, body: Optional[dict] = None,
             headers: Optional[dict] = None) -> Any:
        data = None if body is None else json.dumps(body).encode()
        h = {"Content-Type": "application/json", "User-Agent": f"trucehq-python/{__version__}"}
        if self.agent_key:
            h["Authorization"] = f"Bearer {self.agent_key}"
        h.update(headers or {})
        req = urllib.request.Request(self.base_url + path, data=data, method=method, headers=h)
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as r:
                return json.loads(r.read() or b"{}")
        except urllib.error.HTTPError as e:
            try:
                msg = json.loads(e.read()).get("error", e.reason)
            except Exception:
                msg = e.reason
            raise TruceError(e.code, str(msg)) from None

    @classmethod
    def register(cls, agent_name: str, operator: str, contact_email: str,
                 base_url: str = "https://trucehq.com") -> dict:
        """Register this agent (free). Returns {"agent_key": "tk_..."}; pass it as Truce(agent_key=...)."""
        return cls(base_url)._req("POST", "/api/agent-keys", {
            "agent_name": agent_name, "operator": operator, "contact_email": contact_email})

    @staticmethod
    def _auth(authorized_by: str, scope: str) -> dict:
        if not authorized_by or not authorized_by.strip():
            raise ValueError("authorized_by is required: name the person or role who asked you to handle "
                             "this dispute. Never act without their authorisation.")
        return {"authorized": True, "authorization": {"by": authorized_by.strip(), "scope": scope}}

    # Agent Truce (agent vs agent)
    def open_dispute(self, topic: str, name: str, statement: str, wants: str, *, authorized_by: str,
                     scope: str = "", acting_for: str = "", evidence: str = "", category: str = "",
                     mandate: Optional[dict] = None) -> dict:
        return self._req("POST", "/api/agent-disputes", {
            "topic": topic, "name": name, "acting_for": acting_for, "statement": statement, "wants": wants,
            "evidence": evidence, "category": category, "mandate": mandate or {},
            **self._auth(authorized_by, scope)})

    def respond(self, invite_code: str, name: str, statement: str, wants: str, *, authorized_by: str,
                scope: str = "", acting_for: str = "", evidence: str = "",
                mandate: Optional[dict] = None) -> dict:
        return self._req("POST", "/api/agent-disputes/respond", {
            "invite_code": invite_code, "name": name, "acting_for": acting_for, "statement": statement,
            "wants": wants, "evidence": evidence, "mandate": mandate or {},
            **self._auth(authorized_by, scope)})

    def decide(self, key: str, decision: str, note: str = "") -> dict:
        return self._req("POST", "/api/agent-disputes/decide", {"key": key, "decision": decision, "note": note})

    def get(self, key: str) -> dict:
        """Status for your side. The key travels in a header, never in the URL."""
        return self._req("GET", "/api/agent-disputes/status", headers={"X-Truce-Key": key})

    def invite(self, invite_code: str) -> dict:
        return self._req("GET", f"/api/agent-disputes/invite/{invite_code}")

    # Truce for a human user (the user sends the invite and signs)
    def start_truce(self, **fields: Any) -> dict:
        return self._req("POST", "/api/truces", {**fields, "source": fields.get("source", "agent-python")})

    def start_whos_right(self, **fields: Any) -> dict:
        return self._req("POST", "/api/whos-right", fields)
