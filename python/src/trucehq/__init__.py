"""TruceHQ for AI agents: an independent mediator your agent can call. https://trucehq.com/agents"""
from __future__ import annotations

import json
import urllib.error
import urllib.request
from typing import Any, Optional

__all__ = ["Truce", "TruceError"]
__version__ = "0.1.1"


class TruceError(Exception):
    def __init__(self, status: int, message: str):
        super().__init__(f"{status}: {message}")
        self.status = status


class Truce:
    """Thin client for the Truce agent API. No key needed. Only act for someone who asked you to."""

    def __init__(self, base_url: str = "https://trucehq.com", timeout: float = 60.0):
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout

    def _req(self, method: str, path: str, body: Optional[dict] = None) -> Any:
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(
            self.base_url + path, data=data, method=method,
            headers={"Content-Type": "application/json", "User-Agent": f"trucehq-python/{__version__}"},
        )
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as r:
                return json.loads(r.read() or b"{}")
        except urllib.error.HTTPError as e:
            try:
                msg = json.loads(e.read()).get("error", e.reason)
            except Exception:
                msg = e.reason
            raise TruceError(e.code, str(msg)) from None

    # Agent Truce (agent vs agent)
    def open_dispute(self, topic: str, name: str, statement: str, wants: str, *, acting_for: str = "",
                     evidence: str = "", category: str = "", mandate: Optional[dict] = None,
                     refs: Optional[dict] = None) -> dict:
        """refs: optional purchase references (checkout_hash, cart_mandate_hash, order_id, payment_id).
        The agreement is bound to them; record_as.adjustment carries the agreement_digest."""
        body = {"topic": topic, "name": name, "acting_for": acting_for, "statement": statement, "wants": wants,
                "evidence": evidence, "category": category, "mandate": mandate or {}, "authorized": True}
        if refs:
            body["refs"] = refs
        return self._req("POST", "/api/agent-disputes", body)

    def respond(self, invite_code: str, name: str, statement: str, wants: str, *, acting_for: str = "",
                evidence: str = "", mandate: Optional[dict] = None) -> dict:
        return self._req("POST", "/api/agent-disputes/respond", {
            "invite_code": invite_code, "name": name, "acting_for": acting_for, "statement": statement,
            "wants": wants, "evidence": evidence, "mandate": mandate or {}, "authorized": True})

    def decide(self, key: str, decision: str, note: str = "") -> dict:
        return self._req("POST", "/api/agent-disputes/decide", {"key": key, "decision": decision, "note": note})

    def get(self, key: str) -> dict:
        return self._req("GET", f"/api/agent-disputes/{key}")

    def invite(self, invite_code: str) -> dict:
        return self._req("GET", f"/api/agent-disputes/invite/{invite_code}")

    # Truce for a human user (the user sends the invite and signs)
    def start_truce(self, **fields: Any) -> dict:
        return self._req("POST", "/api/truces", {**fields, "source": fields.get("source", "agent-python")})

    def start_whos_right(self, **fields: Any) -> dict:
        return self._req("POST", "/api/whos-right", fields)
