# TruceEscrow: escrow with a neutral settlement step

A minimal, non-custodial escrow template for agent purchases on EVM chains (Base, Ethereum, Polygon, and others).
MIT licensed. **Unaudited.** The live beta below has small, fixed per-deal caps for that reason.

## Live beta on Ethereum mainnet

- Contract: [`0x2f03a0b6ec53d760a9f2da3be10271e603fef0c9`](https://etherscan.io/address/0x2f03a0b6ec53d760a9f2da3be10271e603fef0c9)
- Deploy transaction: [`0x8bd0f09d…dc2ac`](https://etherscan.io/tx/0x8bd0f09de7956aa7cf1bb6946d41cec19eed9efeb9881a7cb88ca74f36bdc2ac)
- Accepted, with a per-deal cap set once in the constructor: ETH (0.4 ETH), USDC `0xA0b8…eB48` (1,000), USDT `0xdAC1…1ec7` (1,000). Any other token is rejected.
- No owner and no admin functions: nobody, including TruceHQ, can change the caps, pause the contract or touch funds.
- Compiled with solc 0.8.28, optimizer on (200 runs), EVM version cancun.

Test deployment (earlier, uncapped version) on Base Sepolia: `0xe538f22882174fa6fbb781526a4486569d83af47`.

## How it works

The buyer (or the buyer's agent) locks funds for a seller. Money moves only when:

1. the buyer releases it to the seller, or the seller refunds the buyer;
2. the release deadline passes with no dispute, and the seller claims;
3. **both parties sign the same split** (EIP-712), typically the outcome of a TruceHQ agreement, referenced by its `agreement_digest`;
4. a dispute is open and the dispute timeout passes with no signed split: the funds are split evenly, so neither side gains by stalling.

TruceHQ, or any mediator, never holds keys, never signs and never receives funds here. Only the two parties' own signatures can move money. Mediation stays non-binding: the contract pays out only what both sides signed.

## Settling a dispute with TruceHQ

1. Either party calls `dispute(dealId)` before the release deadline.
2. The two agents open an Agent Truce at `https://trucehq.com/mcp` (or `POST https://trucehq.com/api/agent-disputes`), passing the escrow deal as a reference, e.g. `refs.order_id = "<chainId>:<escrow>:<dealId>"`.
3. When both agree, TruceHQ returns the agreement and its `agreement_digest`.
4. Each party signs `Settle(dealId, toSeller, toBuyer, agreementDigest)` with the EIP-712 domain `{ name: "TruceEscrow", version: "1", chainId, verifyingContract }`. Use the SHA-256 `agreement_digest` as the `bytes32` value (drop the `sha256:` prefix and add `0x`).
5. Anyone submits both signatures to `settle(...)`. The `Settled` event records the digest, so the payout points to the agreement.

## Functions

| Function | Who | When |
|---|---|---|
| `open(seller, token, amount, releaseAt, disputeTimeout)` | buyer | locks native coin (`token = 0x0`) or an ERC-20 |
| `release(id)` | buyer | any time before closing |
| `refund(id)` | seller | any time before closing |
| `dispute(id)` | either party | before `releaseAt` |
| `claim(id)` | anyone | after `releaseAt`, if not disputed |
| `settle(id, toSeller, toBuyer, digest, sigBuyer, sigSeller)` | anyone | split signed by both, totals the amount |
| `expire(id)` | anyone | dispute open longer than `disputeTimeout` (min 1 day): splits the funds evenly, so neither side gains by stalling |

## Tests

```
npm install solc@0.8.28 ganache@7 ethers@6
node escrow/test/test.mjs
```

Covers release, claim after deadline, dispute blocking claim, signed split, wrong signer, bad split, replay, even split on dispute timeout and seller refund (native coin), plus open and signed split with three ERC-20 types: standard (USDC-style), no return value (USDT-style) and fee-on-transfer.

Questions: frank@trucehq.com · https://trucehq.com/agents

## Coins and chains

One contract, deployed per chain, works on any EVM chain: Ethereum, Base, Arbitrum, Optimism, Polygon, BNB Chain, Avalanche and more.

- Native coin of the chain: ETH, POL, BNB, AVAX and so on (`token = address(0)`).
- Stablecoins: USDC, USDT (including the USDT that returns nothing on transfer), DAI, PYUSD, EURC and other ERC-20s.
- Bitcoin on EVM chains: WBTC, cbBTC, tBTC.
- Fee-on-transfer tokens work: the deal holds what actually arrived.
- Not supported: rebasing tokens whose balance changes by itself (for example stETH; use wstETH instead).

Bitcoin, Litecoin and Zcash themselves cannot run this contract. The same "both parties sign the split" idea works there as a 2-of-2 multisig with a timelocked fallback; that is a separate template. Solana needs its own program.

Tests compile for the `paris` EVM target only because the local test chain (ganache 7) predates newer opcodes. For deployment use the chain's current default.
