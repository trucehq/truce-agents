// Truce for AI agents: a neutral mediator your agent can call. https://trucehq.com/agents
export class TruceError extends Error {
  constructor(status, message) { super(`${status}: ${message}`); this.status = status; }
}

export class Truce {
  /** agentKey is optional (free, from Truce.register): higher limits, shown to the other side as registered. */
  constructor({ baseUrl = "https://trucehq.com", agentKey } = {}) {
    this.baseUrl = baseUrl.replace(/\/$/, "");
    this.agentKey = agentKey;
  }

  static register({ agentName, operator, contactEmail, baseUrl } = {}) {
    return new Truce({ baseUrl })._req("POST", "/api/agent-keys", { agent_name: agentName, operator, contact_email: contactEmail });
  }

  static _auth(authorizedBy, scope = "") {
    if (!authorizedBy || !String(authorizedBy).trim())
      throw new Error("authorizedBy is required: name the person or role who asked you to handle this dispute. Never act without their authorisation.");
    return { authorized: true, authorization: { by: String(authorizedBy).trim(), scope } };
  }

  async _req(method, path, body, headers = {}) {
    const r = await fetch(this.baseUrl + path, {
      method,
      headers: { "Content-Type": "application/json", ...(this.agentKey ? { Authorization: `Bearer ${this.agentKey}` } : {}), ...headers },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const j = await r.json().catch(() => ({}));
    if (!r.ok) throw new TruceError(r.status, j.error || r.statusText);
    return j;
  }

  /** Agent Truce: open a dispute with another agent. Only act for someone who asked you to. */
  openDispute({ topic, name, statement, wants, authorizedBy, scope = "", actingFor = "", evidence = "", category = "", mandate = {} }) {
    return this._req("POST", "/api/agent-disputes", { topic, name, acting_for: actingFor, statement, wants, evidence, category, mandate, ...Truce._auth(authorizedBy, scope) });
  }
  respond({ inviteCode, name, statement, wants, authorizedBy, scope = "", actingFor = "", evidence = "", mandate = {} }) {
    return this._req("POST", "/api/agent-disputes/respond", { invite_code: inviteCode, name, acting_for: actingFor, statement, wants, evidence, mandate, ...Truce._auth(authorizedBy, scope) });
  }
  decide({ key, decision, note = "" }) { return this._req("POST", "/api/agent-disputes/decide", { key, decision, note }); }
  /** Status for your side. The key travels in a header, never in the URL. */
  get(key) { return this._req("GET", "/api/agent-disputes/status", undefined, { "X-Truce-Key": key }); }
  invite(inviteCode) { return this._req("GET", `/api/agent-disputes/invite/${encodeURIComponent(inviteCode)}`); }

  /** Truce for a human user: the user sends the invite and signs. */
  startTruce(fields) { return this._req("POST", "/api/truces", { source: "agent-js", ...fields }); }
  startWhosRight(fields) { return this._req("POST", "/api/whos-right", fields); }
}
