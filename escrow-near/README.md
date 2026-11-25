# TruceEscrow for NEAR: escrow with a neutral settlement step

A minimal, non-custodial escrow contract for agent purchases on NEAR. Rust with `near-sdk` 5.30.
The same semantics as the EVM TruceEscrow contract and the Solana program.
MIT licensed. **Unaudited: get an independent audit before using it with real funds.**

**Not deployed.** There is no contract account on mainnet or testnet yet. The account ids in this README and the tests are examples.

## How it works

The buyer (or the buyer's agent) locks NEAR or a NEP-141 token for a seller. Money moves only when:

1. the buyer releases it to the seller, or the seller refunds the buyer;
2. the release deadline passes with no dispute, and anyone triggers the seller's claim;
3. **buyer and seller both approve the same split and agreement digest**, typically the outcome of a TruceHQ agreement;
4. a dispute is open and the dispute timeout passes with no agreed split: the funds are split evenly, so neither side gains by stalling.

TruceHQ, or any mediator, never holds keys, never approves and never receives funds here. There is no TruceHQ account in the contract. Only the buyer and seller can move money. Mediation stays non-binding: the contract pays out only what both sides approved.

### Opening a deal

- **NEAR**: the buyer calls `open(seller, coin = "native", amount, release_at, dispute_timeout)` and attaches exactly `amount + 0.01 NEAR`. The deal is funded at once.
- **Token**: two steps.
  1. The buyer calls `open(seller, coin = {"ft": "<token account>"}, amount, release_at, dispute_timeout)` and attaches exactly 0.01 NEAR. The deal is created with status `awaiting_funds` and the call returns its id.
  2. The buyer calls `ft_transfer_call` on the token contract with `receiver_id` = the escrow, the amount and `msg` = the deal id as a string (for example `"7"`). The token contract calls the escrow's `ft_on_transfer`, which funds the deal.

`ft_on_transfer` checks that the calling contract is an accepted token and is the deal's token, that the sender is the deal's buyer, that the deal is still awaiting funds, that the amount is at least the deal amount and that `release_at` has not passed. If any check fails it returns the whole amount as unused, so the token contract gives it back to the sender. If more than the deal amount arrives, the excess is returned. A second funding of the same deal is returned in full.

Times are Unix seconds (`release_at` an absolute time, `dispute_timeout` a duration). The contract compares them with the block timestamp.

### Payouts

Every payout is a transfer (NEAR) or an `ft_transfer` with 1 yoctoNEAR attached (tokens), followed by a callback `on_payout`. The owed amount is cleared when the transfer goes out and is added back if the transfer fails, for example because the recipient is not registered with the token contract or a NEAR recipient account does not exist. The deal stays `closed` with `owed_seller` / `owed_buyer` showing what is still due. Anyone can call `retry_payout(id)` once the problem is fixed; the recipients are fixed in the deal, so nobody can redirect the funds. No funds are lost on a failed transfer.

Calls that pay out (`release`, `refund`, `claim`, `expire`, `retry_payout` and the `approve_settlement` that completes a settlement) should be sent with at least 100 Tgas. Each payout reserves 15 Tgas for `ft_transfer` and 10 Tgas for the callback. If there is not enough gas the whole call fails and nothing changes.

## Settling a dispute with TruceHQ

1. Either party calls `dispute(id)` before the release deadline.
2. The two agents open an Agent Truce at `https://trucehq.com/mcp` (or `POST https://trucehq.com/api/agent-disputes`), passing the escrow deal as a reference, e.g. `refs.order_id = "near:<escrow account>:<id>"`.
3. When both agree, TruceHQ returns the agreement and its `agreement_digest`.
4. Each party calls `approve_settlement(id, to_seller, to_buyer, agreement_digest)` from its own account. Use the SHA-256 `agreement_digest` as 64 hex characters (drop the `sha256:` prefix). The split must total the escrowed amount.
5. The second matching approval pays out in the same call. The `settled` event records the split and the digest, so the payout points to the agreement.

Each party can replace its approval by calling `approve_settlement` again, or remove it with `withdraw_approval(id)`, until the deal is paid out. Approvals that do not match (a different split or a different digest) pay nothing. Approvals are cleared when the deal closes, so they cannot be replayed.

### Why two approvals, not signed messages

On EVM the parties sign an EIP-712 message off-chain and the contract checks it with `ecrecover`. On NEAR the simpler secure equivalent is that each party calls the contract itself:

- The protocol has already authenticated the caller (`predecessor_account_id`) before the contract runs. No signature parsing, no key formats, no message encoding to get wrong.
- It works for any account, including multisigs and other contracts acting as agent wallets, and for accounts whose access keys rotate.
- The parties do not need to be online at the same time, and there is no expiring transaction to coordinate.

## Methods

| Method | Who | When |
|---|---|---|
| `new(caps)` | the contract account itself, once | at deployment |
| `open(seller, coin, amount, release_at, dispute_timeout)` | buyer | attach `amount + 0.01 NEAR` (NEAR) or `0.01 NEAR` (token). `release_at` in the future, `dispute_timeout` at least 1 day (86,400 s), `amount` at most the coin's cap |
| `ft_on_transfer(sender_id, amount, msg)` | an accepted token contract, via the buyer's `ft_transfer_call` | funds a token deal; `msg` is the deal id |
| `release(id)` | buyer | any time before closing |
| `refund(id)` | seller | any time before closing |
| `dispute(id)` | buyer or seller | before `release_at`, if not already disputed |
| `claim(id)` | anyone | after `release_at`, if not disputed: pays the seller |
| `expire(id)` | anyone | dispute open longer than `dispute_timeout`: splits the funds evenly (the odd unit goes to the buyer) |
| `approve_settlement(id, to_seller, to_buyer, agreement_digest)` | buyer or seller | before closing; pays out when both approvals match |
| `withdraw_approval(id)` | buyer or seller | before closing |
| `retry_payout(id)` | anyone | after a failed payout |
| `on_payout(id, to_seller, amount)` | the contract itself | payout callback |
| `get_deal(id)`, `get_caps()`, `max_amount(coin)`, `deal_storage_deposit()`, `next_id()` | view | |

Amounts are JSON strings (`U128`). A coin is `"native"` or `{"ft": "<token account id>"}`.

Errors are panics whose message starts with `BadState`, `NotParty`, `TooEarly`, `TooLate`, `BadSplit`, `BadDigest`, `NotAccepted` or `Private method`.

Events follow NEP-297 (`EVENT_JSON:` logs, standard `truce_escrow`, version `1.0.0`): `opened`, `disputed`, `released`, `refunded`, `expired`, `settlement_approved`, `approval_withdrawn`, `settled`, `payout_failed`.

## Coins

- **NEAR**, native, amounts in yoctoNEAR (10^24 per NEAR).
- **NEP-141 tokens**, for example native **USDC** on NEAR (`17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1`, 6 decimals) and **USDT** on NEAR (`usdt.tether-token.near`, 6 decimals). Check the token account ids against the issuers' own pages before deployment.

Token notes:

- USDC and USDT on NEAR have issuer controls (pause, blocklist, upgradable contract). The issuer could stop a payout. This is the same issuer risk as on other chains.
- Bridged tokens carry the bridge's risk as well.

## Beta caps

The contract only accepts the coins listed in `new(caps)`, each with a maximum deal size, for example:

```json
{"caps": [
  {"coin": "native", "max_amount": "100000000000000000000000000"},
  {"coin": {"ft": "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1"}, "max_amount": "1000000000"},
  {"coin": {"ft": "usdt.tether-token.near"}, "max_amount": "1000000000"}
]}
```

(100 NEAR, 1,000 USDC, 1,000 USDT.) The list must not be empty, each cap must be above zero and a coin may appear only once. A deal above the cap, or in a coin not on the list, is rejected. Tokens from any other contract are returned by `ft_on_transfer`.

The caps are immutable. There is no owner, no admin and no method that changes the caps or upgrades the code. On NEAR, an account with an access key can always redeploy its own code, so the contract is only immutable once the escrow account has **no access keys**. Deploy it like this, in one transaction signed by the escrow account's own key, sent to itself:

1. `DeployContract` with the wasm;
2. `FunctionCall new(caps)` (only the account itself may call `new`);
3. `DeleteKey` for that same key.

Afterwards `view_access_key_list` for the account must be empty, and anyone can check it. To change the caps or the code, deploy a new contract on a new account.

## Storage deposits

NEAR charges for contract state by locking NEAR on the account (storage staking).

- **Per deal**: the buyer attaches 0.01 NEAR with `open`. That pays for 1,000 bytes; a deal record with both approvals stored uses at most 457 bytes (64-character account ids, measured in the tests). The deal record is kept as a permanent on-chain receipt, so this deposit is not refunded. The rest of the 0.01 NEAR leaves room for the 1 yoctoNEAR each `ft_transfer` needs.
- **Contract code**: whoever deploys funds the escrow account with enough NEAR to cover the wasm and the caps (about 1 NEAR per 100 KB). Once the keys are deleted this NEAR stays locked for good.
- **Token registration**: NEP-141 tokens hold balances only for registered accounts. Before the first token deal, someone calls `storage_deposit` on the token contract for the escrow account (about 0.00125 NEAR for USDC and USDT). The seller and buyer must be registered with the token too, or their payout fails and stays owed until they register and anyone calls `retry_payout`.

## Build and test

```
cargo test
```

The tests are unit tests with `near-sdk`'s mocked blockchain (`VMContextBuilder`, `testing_env!`). They call the contract methods directly and inspect the receipts and logs the contract creates. They cover init rules, caps for NEAR and tokens, unlisted and wrong tokens, open validation, token funding (excess, short amount, wrong sender, second funding, past deadline, fake token), release, refund, claim by anyone after the deadline, dispute rules, the even split with an odd amount, settle with matching approvals, mismatched split or digest, withdrawn approval, stranger, bad digest, replay after closing, approvals per deal, failed payouts restored and retried, private methods, and the worst-case storage of a deal.

Not covered: the deployed wasm against a real token contract. `near-workspaces` sandbox tests have not been written, because the wasm has not been built yet.

Building the wasm for deployment:

```
rustup target add wasm32-unknown-unknown
cargo install cargo-near
cargo near build
```

Plain `cargo build` is refused by `near-sdk` 5.30 on purpose. Use a Rust toolchain that `cargo-near` supports for NEAR contracts; very new compilers emit wasm features that the NEAR runtime may reject. That build has not been done for this code yet.

Questions: frank@trucehq.com · https://trucehq.com/agents
