# TruceEscrow for Solana: escrow with a neutral settlement step

A minimal, non-custodial escrow program for agent purchases on Solana. Native Rust (`solana-program` 2.3), no Anchor.
The same semantics as the EVM TruceEscrow contract.
MIT licensed. **Unaudited: get an independent audit before using it with real funds.**

**Not deployed.** There is no program id on any cluster yet. The id in the tests is a placeholder.

## How it works

The buyer (or the buyer's agent) locks SOL or an SPL token for a seller. Money moves only when:

1. the buyer releases it to the seller, or the seller refunds the buyer;
2. the release deadline passes with no dispute, and anyone triggers the seller's claim;
3. **buyer and seller both sign the same split**, typically the outcome of a TruceHQ agreement, referenced by its `agreement_digest`;
4. a dispute is open and the dispute timeout passes with no signed split: the funds are split evenly, so neither side gains by stalling.

TruceHQ, or any mediator, never holds keys, never signs and never receives funds here. Only the two parties' own signatures can move money. Mediation stays non-binding: the program pays out only what both sides signed.

### Where the funds sit

Each deal is a program-owned PDA with seeds `["deal", buyer, deal_id]` (`deal_id` is a `u64` the buyer picks, little endian).

- **SOL**: the lamports sit in the deal account itself, on top of its rent. Only this program can debit it.
- **SPL tokens**: the tokens sit in the vault, the associated token account of the deal PDA for that mint. Its owner is the deal PDA, so only this program can sign for it. `open` creates the vault (idempotently, so a vault someone created or funded in advance does not block the deal).

When a deal closes, the deal account is wiped and its rent goes back to the buyer, who paid it. An empty classic SPL Token vault is closed too, with its rent to the buyer. A Token-2022 vault, or a vault holding tokens someone donated, is left open, because closing it could fail and must never block a payout.

Anyone can send lamports to a deal address before it is opened; `open` still works.

## Settling a dispute with TruceHQ

1. Either party sends `dispute` before the release deadline.
2. The two agents open an Agent Truce at `https://trucehq.com/mcp` (or `POST https://trucehq.com/api/agent-disputes`), passing the escrow deal as a reference, e.g. `refs.order_id = "solana:<programId>:<dealAddress>"`.
3. When both agree, TruceHQ returns the agreement and its `agreement_digest`.
4. One party builds a single transaction with the instruction `settle(toSeller, toBuyer, agreementDigest)`, with both buyer and seller marked as signers. Use the SHA-256 `agreement_digest` as the 32 bytes (drop the `sha256:` prefix and hex-decode). That party signs, then passes the partly signed transaction to the other party, who checks the split and the digest and adds the second signature. Either one submits it.
5. The program logs `Settled`, the deal address, both amounts and the digest (`sol_log_data`), so the payout points to the agreement.

### Why both signers, not signed messages

On EVM the parties sign an EIP-712 message off-chain and the contract checks it with `ecrecover`. On Solana the simpler secure equivalent is to require both buyer and seller as signers of the `settle` transaction:

- The runtime verifies both ed25519 signatures over the whole transaction message before the program runs. The message includes the program id, the deal account, the split and the agreement digest, so a signature cannot be reused for another deal, another split or another program.
- Replay is impossible: a transaction carries a recent blockhash and the runtime refuses a signature it has already processed, and the deal is closed by the first `settle` anyway.
- No signature parsing in the program, so none of the known pitfalls of ed25519 instruction introspection (wrong offsets, checking a different instruction than the one signed, message mismatches).
- Buyers or sellers that are themselves programs (PDAs, e.g. agent wallets or multisigs) can sign through `invoke_signed`. They could not produce an ed25519 signature.

The cost: both parties must sign the same transaction. A recent blockhash expires after about a minute, so for parties that sign hours apart, build the transaction with a durable nonce account instead.

## Instructions

Encoding: one tag byte, then little-endian fields. Builders are in `src/instruction.rs`.

| Tag | Instruction | Who signs | When |
|---|---|---|---|
| 0 | `open(dealId, amount, releaseAt, disputeTimeout)` | buyer | locks SOL (4 accounts) or an SPL token (9 accounts). `releaseAt` in the future, `disputeTimeout` at least 1 day |
| 1 | `dispute` | buyer or seller | before `releaseAt` |
| 2 | `release` | buyer | any time before closing |
| 3 | `refund` | seller | any time before closing |
| 4 | `claim` | anyone | after `releaseAt`, if not disputed |
| 5 | `expire` | anyone | dispute open longer than `disputeTimeout`: splits the funds evenly (the odd unit goes to the buyer) |
| 6 | `settle(toSeller, toBuyer, agreementDigest)` | buyer and seller | the split must total the escrowed amount |

Accounts for `open`: buyer (signer, writable), seller, deal PDA (writable), system program; for a token deal also mint, vault (writable), buyer's token account (writable), token program, associated token account program.

Accounts for `release`, `refund`, `claim`, `expire` and `settle`: deal PDA (writable), buyer (writable), seller (writable); for a token deal also vault, buyer's token account, seller's token account (all writable), mint, token program. The recipient token accounts must be owned by the buyer and seller for the deal's mint, so whoever submits `claim` or `expire` cannot redirect the funds.

Times are Unix seconds from the `Clock` sysvar. Program errors (`Custom(n)`): 0 BadState, 1 NotParty, 2 TooEarly, 3 TooLate, 4 BadSplit, 5 BadAccount, 6 TransferFailed, 7 InvalidInstruction, 8 Overflow, 9 Capped.

## Beta caps

Like the EVM contract, this unaudited program caps every deal: at most **5 SOL**, or at most **1,000 USDC or USDT** (classic SPL Token). Other tokens are refused with `Capped` (9) for now; the sections below describe what the code handles once the allowlist is widened. Tests build with `--features any-mint` so test mints pass the allowlist (the amount cap still applies).

## Coins supported

- **SOL**, native.
- **SPL tokens** on the classic Token program, including **USDC** and **USDT** on Solana, and any other SPL mint.
- **Token-2022** mints, including **PYUSD**. Transfers use `TransferChecked`, which both token programs accept.

Token-2022 notes:

- Transfer fees work: the deal holds what actually arrived in the vault, and the payout is charged the fee again on the way out.
- Transfer hooks are not supported: the hook's extra accounts are not passed, so `open` fails while a hook program is set. If the mint's hook authority sets a hook program after a deal is opened, payouts fail until it is removed.
- A mint with a permanent delegate lets its issuer move tokens out of any account, the vault included. Only escrow such tokens if you trust the issuer.
- A freeze authority (USDC and USDT have one) can freeze the vault or a recipient account and so stop a payout. This is the same issuer risk as the blocklist on EVM stablecoins.
- Non-transferable mints cannot be escrowed.

SOL payouts are credited directly. A recipient wallet with no SOL at all can only receive at least the rent-exempt minimum (about 0.00089 SOL), so very small SOL deals should go to funded wallets.

## Build and test

```
cargo test --features any-mint
```

The tests run on the host: the program runs natively inside `solana-program-test` 2.3 (`processor!` mode) and calls the real SPL Token 3.5, Token-2022 8.0 and Associated Token Account 1.1.1 programs that ship with it.

Covered: release, refund, claim after the deadline by anyone, dispute blocking claim, dispute rules (stranger, twice, too late), even split on dispute timeout with an odd amount, signed split, missing second signer, wrong signer, bad split, replay, open validation, opening on a pre-funded deal address, wrong recipient (SOL), plus classic SPL Token settle with vault close, redirected token account on claim, donation to the vault, Token-2022 expire, Token-2022 transfer fee, and fake token program or wrong vault on open.

Building the on-chain program (`.so`) needs `cargo build-sbf` on a machine with the Solana toolchain installed. That has not been done for this code yet.

Questions: frank@trucehq.com · https://trucehq.com/agents
