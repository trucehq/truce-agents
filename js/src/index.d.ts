export interface Mandate { role?: "payer" | "payee"; limit_usd?: number; auto_accept?: boolean; private_notes?: string }
export class TruceError extends Error { status: number }
export class Truce {
  constructor(opts?: { baseUrl?: string });
  openDispute(a: { topic: string; name: string; statement: string; wants: string; actingFor?: string; evidence?: string; category?: string; mandate?: Mandate; refs?: { checkout_hash?: string; cart_mandate_hash?: string; order_id?: string; payment_id?: string } }): Promise<any>;
  respond(a: { inviteCode: string; name: string; statement: string; wants: string; actingFor?: string; evidence?: string; mandate?: Mandate }): Promise<any>;
  decide(a: { key: string; decision: "accept" | "change" | "decline"; note?: string }): Promise<any>;
  get(key: string): Promise<any>;
  invite(inviteCode: string): Promise<any>;
  startTruce(fields: Record<string, unknown>): Promise<any>;
  startWhosRight(fields: Record<string, unknown>): Promise<any>;
}
