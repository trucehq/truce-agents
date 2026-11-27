# TruceEscrow for Starknet: escrow with a neutral settlement step

A minimal, non-custodial escrow contract for agent purchases on Starknet. Cairo 2.18, OpenZeppelin Contracts for Cairo 4.0.1 for the SNIP-12 and SRC-6 helpers.
The same semantics as the EVM TruceEscrow contract.
MIT licensed.

**Status: Not yet deployed; unaudited.** Get an independent audit before using it with real funds.

## How it works

The buyer (or the buyer's agent) locks an ERC-20 token (STRK, USDC and others) for a seller. Money moves only when:

1. the buyer releases it to the seller, or the seller refunds the buyer;
2. the release deadline passes with no dispute, and anyone triggers the seller's claim;
3. **buyer and seller both sign the same split** (SNIP-12 typed data), typically the outcome of a TruceHQ agreement, referenced by its `agreement_digest`;
4. a dispute is open and the dispute timeout passes with no signed split: the funds are split evenly, so neither side gains by stalling.

TruceHQ, or any mediator, never holds keys, never signs and never receives funds here. Only the two parties' own signatures can move money. Mediation stays non-binding: the contract pays out only what both sides signed.

No owner, no admin functions, no upgrade path (no `replace_class`). The accepted tokens and the per-deal cap for each are set once in the constructor and cannot be changed.

### Where the funds sit

One contract holds every deal's tokens in its own ERC-20 balance and tracks them per deal (`deals: Map<u64, Deal>`). Deal ids count up from 1. The buyer `approve`s the escrow, then calls `open`, which pulls the tokens with `transfer_from`; on Starknet both calls fit in one multicall transaction.

Every payout closes the deal **before** any token transfer (checks-effects-interactions), and the state-changing entry points that move tokens also hold a reentrancy lock. A token that calls back into the escrow during a payout is refused (`Reentrant`), and the whole transaction reverts.

## Settling a dispute with TruceHQ

1. Either party calls `dispute(id)` before the release deadline.
2. The two agents open an Agent Truce at `https://trucehq.com/mcp` (or `POST https://trucehq.com/api/agent-disputes`), passing the escrow deal as a reference, e.g. `refs.order_id = "starknet:<chainId>:<escrow>:<dealId>"`.
3. When both agree, TruceHQ returns the agreement and its `agreement_digest`.
4. Each party signs the SNIP-12 typed data below with their own Starknet account (Argent, Braavos, OpenZeppelin or any SRC-6 account), e.g. `account.signMessage(typedData)` in starknet.js. Use the SHA-256 `agreement_digest` as a `u256`: drop the `sha256:` prefix; the first 16 bytes are `high`, the last 16 are `low`.
5. Anyone submits both signatures to `settle(...)`. The `Settled` event records the digest, so the payout points to the agreement.

```json
{
  "types": {
    "StarknetDomain": [
      { "name": "name", "type": "shortstring" },
      { "name": "version", "type": "shortstring" },
      { "name": "chainId", "type": "shortstring" },
      { "name": "revision", "type": "shortstring" }
    ],
    "Settle": [
      { "name": "escrow", "type": "ContractAddress" },
      { "name": "dealId", "type": "u128" },
      { "name": "toSeller", "type": "u256" },
      { "name": "toBuyer", "type": "u256" },
      { "name": "agreementDigest", "type": "u256" }
    ]
  },
  "primaryType": "Settle",
  "domain": { "name": "TruceEscrow", "version": "v1", "chainId": "SN_MAIN", "revision": "1" },
  "message": {
    "escrow": "<escrow address>",
    "dealId": 7,
    "toSeller": { "low": "150000000", "high": "0" },
    "toBuyer": { "low": "100000000", "high": "0" },
    "agreementDigest": { "low": "0xa3bf4f1b2b0b822cd15d6c15b0f00a08", "high": "0x9f86d081884c7d659a2feaa0c55ad015" }
  }
}
```

The domain version is `"v1"`, not `"1"`: starknet.js reads the string `"1"` as the number 1, while Cairo's `'1'` is the short string `0x31`, so a numeric-looking version would hash differently in the wallet and in the contract. `chainId` is `SN_MAIN` on mainnet and `SN_SEPOLIA` on Sepolia. A test pins the contract's hash to the one starknet.js 7.6.4 computes for this typed data.

The view `settle_hash(id, toSeller, toBuyer, agreementDigest, signer)` returns the exact hash each party's account must accept, for clients that sign hashes directly.

### Why account signatures (SRC-6), not ecrecover

On EVM the parties sign an EIP-712 message and the contract recovers the signer with `ecrecover`. Starknet has native account abstraction: every party is an account contract, and only that contract knows what a valid signature is (a Stark key, a secp256r1 passkey, a multisig, a guardian). So the contract asks each party's own account:

- The hash each party signs follows SNIP-12 revision 1 (Poseidon), computed with OpenZeppelin's `OffchainMessageHash`: `poseidon('StarkNet Message', domainHash, signer, hashStruct(Settle))`. It binds the chain id, the escrow address, the deal id, the split and the agreement digest, plus the signer's own address, so a buyer's signature can never pass as the seller's.
- `settle` calls `is_valid_signature(hash, signature)` on the buyer's account and on the seller's account (via OpenZeppelin's `assert_valid_signature`, which accepts `'VALID'` and the legacy `1`). Both must accept, or the call fails with `BadSignature`.
- SNIP-12 domains have no `verifyingContract`, so the escrow address is a field of the signed message: a signature for one deployment cannot be used on another.
- Replay: the deal is closed by the first successful `settle` (or any other payout), so the same signatures fail with `BadState`; on another deal or another escrow they fail with `BadSignature`.
- Signatures are not parsed by the escrow, so no malleability or length pitfalls live here; they are the account's rules. That also means a party's account decides what counts as its signature (for example, a session key the account accepts).

Unlike the Solana port, the parties do not have to sign the same transaction: they sign the typed data independently, at any time, and anyone submits both.

## Functions

| Function | Who | When |
|---|---|---|
| `open(seller, token, amount, releaseAt, disputeTimeout) -> id` | buyer | locks an accepted ERC-20 (after `approve`). `releaseAt` in the future, `disputeTimeout` at least 1 day, `amount` at most the token's cap |
| `dispute(id)` | buyer or seller | before `releaseAt` |
| `release(id)` | buyer | any time before closing |
| `refund(id)` | seller | any time before closing |
| `claim(id)` | anyone | after `releaseAt`, if not disputed; pays the seller |
| `expire(id)` | anyone | dispute open at least `disputeTimeout`: splits the funds evenly (the odd unit goes to the buyer) |
| `settle(id, toSeller, toBuyer, agreementDigest, sigBuyer, sigSeller)` | anyone | split signed by both, totals the escrowed amount |
| `get_deal(id)`, `next_id()`, `max_amount(token)`, `settle_hash(...)` | anyone | views |

Events: `Opened`, `Disputed`, `Released` (release and claim), `Refunded`, `Expired`, `Settled`, with the deal id as a key.

Times are Unix seconds from the block timestamp. Errors (short strings): `BadState`, `NotParty`, `TooEarly`, `TooLate`, `BadSplit`, `BadSignature`, `TransferFailed`, `NotAccepted`, `Reentrant`.

Constructor: `constructor(tokens: Span<ContractAddress>, caps: Span<u256>)`. Each listed token gets a fixed per-deal cap; unlisted tokens are refused with `NotAccepted`. No caps have been chosen yet, because nothing has been deployed.

## How it maps to the EVM version

| EVM `TruceEscrow.sol` | Starknet `TruceEscrow` |
|---|---|
| `uint256` deal ids | `u64` deal ids (from 1) |
| native coin (`token = address(0)`) or ERC-20 | ERC-20 only. STRK and ETH are ERC-20 contracts on Starknet, so nothing is lost |
| `maxAmount` per coin set in the constructor | the same, `max_amount` |
| EIP-712 `Settle(dealId, toSeller, toBuyer, agreementDigest)`, domain `{name, version, chainId, verifyingContract}` | SNIP-12 rev 1 `Settle(escrow, dealId, toSeller, toBuyer, agreementDigest)`, domain `{name: TruceEscrow, version: v1, chainId, revision: 1}`; the escrow address moves from the domain into the message |
| `ecrecover`, 65-byte signatures, low-s check | the party's account contract checks the signature (SRC-6 `is_valid_signature`) |
| `nonReentrant` modifier | `lock`/`unlock` storage flag, same functions covered |
| state before transfer in `_close` | the same |
| balance before/after on `open` (fee-on-transfer) | the same |
| `_call` tolerating tokens that return nothing | not needed: Starknet ERC-20s return `bool`; `false` fails with `TransferFailed` |
| custom errors | short-string panics with the same names |

Everything else (who may call what, when, the even split on expiry with the odd unit to the buyer, dispute only before the deadline, release and refund allowed while disputed) is unchanged.

## Coins supported

- **STRK** and **ETH**, both ERC-20 contracts on Starknet.
- **USDC**, **USDT** and other ERC-20s that implement the standard snake_case interface (`transfer`, `transfer_from`, `balance_of`, returning `bool`). Check this before listing a token in the constructor: a legacy token that only exposes camelCase entry points (`transferFrom`, `balanceOf`) is not supported.
- Fee-on-transfer tokens work: the deal holds what actually arrived.
- Not supported: rebasing tokens whose balance changes by itself.
- An issuer that can freeze or blocklist addresses can stop a payout; that is the same issuer risk as on EVM.

## Build and test

Built and tested with Scarb 2.18.0 (Cairo 2.18.0), Starknet Foundry (snforge) 0.59.0 and universal-sierra-compiler 2.11.0.

```
scarb build
snforge test
```

The tests deploy the escrow with OpenZeppelin's ERC-20 and account presets: buyer and seller are real OpenZeppelin account contracts with Stark-curve keys, so every `settle` test goes through real `is_valid_signature` checks. A small mock token in `tests/mocks.cairo` covers fee-on-transfer and a token that re-enters the escrow.

Covered (26 tests): open and fund, open validation (seller, self-deal, zero amount, past deadline, short timeout, unlisted token, cap), constructor validation, open without allowance, release, refund, release and refund while disputed, non-party calls, claim after the deadline by anyone, claim too early, dispute blocking claim, dispute rules (stranger, twice, too late), even split on dispute timeout with an odd amount, signed split with agreement digest, signed split without a dispute, SNIP-12 hash structure, a starknet.js test vector, one-sided signatures (only one party, same signature twice, swapped, buyer signing for the seller), bad signatures (stranger, garbage, wrong length, different split or digest, other chain), bad split and overflowing split, replay on the same deal, on another deal and on another escrow deployment, settle after a claim, fee-on-transfer token, reentrant token, and events.

Dependencies come from git tags (OpenZeppelin `v4.0.1`, Starknet Foundry `v0.59.0`). The `[patch.scarbs-xyz]` entry in `Scarb.toml` only redirects OpenZeppelin's registry reference to `snforge_std` to the same git tag; it can be removed where scarbs.xyz is reachable. With `snforge_std` from git, the first `snforge test` compiles its Cairo plugin with Cargo, so a Rust toolchain is needed.

Deployment (declare and deploy with `sncast`) has not been done.

## License

MIT, see `LICENSE`.

Questions: frank@trucehq.com · https://trucehq.com/agents
