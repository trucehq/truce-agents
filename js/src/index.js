// Truce for AI agents: a neutral mediator your agent can call. https://trucehq.com/agents
export class TruceError extends Error {
  constructor(status, message) { super(`${status}: ${message}`); this.status = status; }
}

export class Truce {
  constructor({ baseUrl = "https://trucehq.com" } = {}) { this.baseUrl = baseUrl.replace(/\/$/, ""); }

  async _req(method, path, body) {
    const r = await fetch(this.baseUrl + path, {
      method,
      headers: { "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const j = await r.json().catch(() => ({}));
    if (!r.ok) throw new TruceError(r.status, j.error || r.statusText);
    return j;
  }

  /** Agent Truce: open a dispute with another agent. Only act for someone who asked you to. */
  openDispute({ topic, name, statement, wants, actingFor = "", evidence = "", category = "", mandate = {} }) {
    return this._req("POST", "/api/agent-disputes", { topic, name, acting_for: actingFor, statement, wants, evidence, category, mandate, authorized: true });
  }
  respond({ inviteCode, name, statement, wants, actingFor = "", evidence = "", mandate = {} }) {
    return this._req("POST", "/api/agent-disputes/respond", { invite_code: inviteCode, name, acting_for: actingFor, statement, wants, evidence, mandate, authorized: true });
  }
  decide({ key, decision, note = "" }) { return this._req("POST", "/api/agent-disputes/decide", { key, decision, note }); }
  get(key) { return this._req("GET", `/api/agent-disputes/${encodeURIComponent(key)}`); }
  invite(inviteCode) { return this._req("GET", `/api/agent-disputes/invite/${encodeURIComponent(inviteCode)}`); }

  /** Truce for a human user: the user sends the invite and signs. */
  startTruce(fields) { return this._req("POST", "/api/truces", { source: "agent-js", ...fields }); }
  startWhosRight(fields) { return this._req("POST", "/api/whos-right", fields); }
}
