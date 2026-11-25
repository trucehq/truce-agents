# TruceEscrowTron: escrow with a neutral settlement step, on Tron

Tron (TVM) port of TruceEscrow, a minimal, non-custodial escrow template for agent purchases.
MIT licensed.

**Unaudited, beta caps. Not deployed.** Get an independent audit before using it with real funds.

## How it works

The buyer (or the buyer's agent) locks TRX or a TRC-20 token for a seller. Money moves only when:

1. the buyer releases it to the seller, or the seller refunds the buyer;
2. the release deadline passes with no dispute, and the seller claims;
3. **both parties sign the same split** (TIP-712, Tron's version of EIP-712), typically the outcome of a TruceHQ agreement, referenced by its `agreement_digest`;
4. a dispute is open and the dispute timeout passes with no signed split: the funds are split evenly, so neither side gains by stalling.

TruceHQ, or any mediator, never holds keys, never signs and never receives funds here. Only the two parties' own signatures can move money.

## Beta caps

The constructor takes `address[] tokens, uint256[] caps`. Only listed coins are accepted and each deal is capped per coin. `address(0)` means TRX. `open()` reverts with `NotAccepted` if the coin is not listed or the amount is above its cap. The list is immutable: there is no owner, no admin key and no way to change it after deployment. To raise caps, deploy a new contract.

Amounts are in the smallest unit: sun for TRX (1 TRX = 1,000,000 sun), and 6 decimals for USDT on Tron (1 USDT = 1,000,000).

Example constructor arguments (100 TRX and 100 USDT per deal):

```
tokens = ["T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb", "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t"]
caps   = [100000000, 100000000]
```

`T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb` is the base58 form of the zero address (hex `410000000000000000000000000000000000000000`), which stands for TRX. `TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t` is USDT on Tron mainnet (hex `41a614f803b6fd780986a42c78ec9c7f77e6ded13c`). On Nile testnet use a test token address instead.

## Functions

| Function | Who | When |
|---|---|---|
| `open(seller, token, amount, releaseAt, disputeTimeout)` | buyer | locks TRX (`token = address(0)`, send `amount` sun as call value) or a listed TRC-20 (approve first) |
| `release(id)` | buyer | any time before closing |
| `refund(id)` | seller | any time before closing |
| `dispute(id)` | either party | before `releaseAt` |
| `claim(id)` | anyone | after `releaseAt`, if not disputed |
| `settle(id, toSeller, toBuyer, digest, sigBuyer, sigSeller)` | anyone | split signed by both, totals the amount |
| `expire(id)` | anyone | dispute open longer than `disputeTimeout` (min 1 day): splits the funds evenly |
| `eip712Domain()` | read | name, version, chain id and contract to sign against |

## What is different from the EVM version

### TRC-20 transfers: checked by balance, not by return value

USDT on Tron (`TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t`) is widely reported to return `false` from `transfer()` even when the transfer succeeded. I believe this is right but have not verified it against the deployed bytecode here, and I do not know whether `transferFrom()` behaves the same way. The EVM version of TruceEscrow rejects a token that returns `false`, so with that token on Tron every deal would fail. In a local check, the EVM contract reverted on `open()` against a mock that returns `false` on success.

So this version does not trust return values at all:

- `open()` requires the `transferFrom` call not to revert, then counts what actually arrived in the contract's balance. Zero arrived means `TransferFailed`. This also handles fee-on-transfer tokens.
- Every payout requires the `transfer` call not to revert, then requires the contract's own balance to have dropped by exactly the amount paid. Less means the transfer did not happen; more would eat into other deals' funds. Either way the payout reverts and the deal stays open, so funds are not lost.
- The check is on the contract's own balance, not the recipient's, so a token that takes a fee from what the recipient receives still works (the recipient gets the amount minus the fee, as with any transfer of that token).

Trade-off: a token that returns `false` and really failed, but still changes the balance, cannot be told apart. That is not a known pattern, and only listed tokens are accepted anyway.

Other USDT on Tron points to be aware of (from general knowledge, not verified here):

- USDT has an issuer blacklist and can be paused. If the buyer or seller is blacklisted, or USDT is paused, any payout to them reverts and the deal cannot close by `release`, `refund`, `claim` or `expire`. Both parties can still sign a `settle` split that sends nothing to the blacklisted address. The EVM version has the same limitation.
- The Tether contract has a transfer-fee setting that is believed to be zero. If it were turned on, the balance checks above still hold.

### TIP-712 settle signatures

The settle message and domain hash are the same as EIP-712: domain `{ name: "TruceEscrow", version: "1", chainId, verifyingContract }`, message `Settle(uint256 dealId, uint256 toSeller, uint256 toBuyer, bytes32 agreementDigest)`. In TIP-712, `address` values (here `verifyingContract`) are written as Tron base58 addresses and hashed as the 20-byte address without the `41` prefix, which is what the contract uses. On the TVM, `ecrecover` returns that same 20-byte address, so it compares directly to the stored buyer and seller.

The tests sign with tronweb's `Trx._signTypedData` using base58 addresses and confirm the signature is identical to one made with ethers using hex addresses, and that the contract accepts it.

**Chain id.** Do not hard-code it. Read it from the contract: `eip712Domain()` returns the chain id that is baked into `DOMAIN_SEPARATOR` (`block.chainid` at deployment, also exposed as `DOMAIN_CHAIN_ID`). Commonly cited values, to verify:

| Network | chainId | hex |
|---|---|---|
| Tron mainnet | 728126428 | 0x2b6653dc |
| Nile testnet | 3448148188 | 0xcd8690dc |
| Shasta testnet | 2494104990 | 0x94a9059e |

These are the last 4 bytes of each network's genesis block hash. Older TVM versions are reported to have returned the full genesis hash from `block.chainid`; reading the value from the contract makes the signature match whatever the TVM actually reported at deployment. TronLink may check the domain chain id against the network it is connected to, so confirm on Nile that the value the contract reports is the one TronLink accepts.

**How a Tron wallet produces a settle signature** (TronLink in the browser, or tronweb with a key in a backend agent):

```js
const E = await tronWeb.contract().at(ESCROW);              // base58 address
const d = await E.eip712Domain().call();
const domain = { name: d.name, version: d.version, chainId: d.chainId.toString(),
                 verifyingContract: tronWeb.address.fromHex(d.verifyingContract) };
const types = { Settle: [
  { name: 'dealId', type: 'uint256' }, { name: 'toSeller', type: 'uint256' },
  { name: 'toBuyer', type: 'uint256' }, { name: 'agreementDigest', type: 'bytes32' } ] };
const value = { dealId: '7', toSeller: '600000', toBuyer: '400000',
                agreementDigest: '0x' + agreementDigestHex };     // TruceHQ sha256 digest, no "sha256:" prefix
// In TronLink: window.tronWeb.trx._signTypedData(domain, types, value)
// In a backend: TronWeb.Trx._signTypedData(domain, types, value, privateKey)
const sig = await tronWeb.trx._signTypedData(domain, types, value);
```

Each party signs the same values. Anyone then submits both signatures with `settle(dealId, toSeller, toBuyer, agreementDigest, sigBuyer, sigSeller)`. Signatures with `v` of 0/1 or 27/28 are both accepted; high-s signatures are rejected.

### Energy and bandwidth instead of gas

Tron charges **energy** for contract execution and **bandwidth** for transaction size, instead of gas. Each account gets a small free daily bandwidth allowance. Energy comes from staking TRX, or else TRX is burned to pay for it. Every contract call needs a `fee_limit` (maximum TRX to burn); set it high enough or the call fails and the energy spent so far is still charged.

- The deployer can set `consume_user_resource_percent` and `origin_energy_limit` at deployment to have the contract owner's staked energy pay part of each call. With 100, callers pay all of it.
- USDT calls (`approve`, and the transfers inside `open` and every payout) are the expensive part. A transfer to an address that has never held USDT is reported to cost noticeably more energy than one to an existing holder.
- `settle` costs two `ecrecover`s plus up to two token transfers. Estimate on Nile with `triggerConstantContract` before setting fee limits for mainnet.
- Sending TRX to an account that has never been activated is reported to cost extra; not measured here.

No energy figures are given here because none were measured.

### Other TVM notes

- TRX amounts use `msg.value` in sun, as on the EVM.
- TRC-10 tokens are not supported. Do not attach TRC-10 tokens to any call (`callTokenValue`); they would be stuck.
- `block.timestamp` is in seconds in Solidity on the TVM (to verify), so `releaseAt` and `disputeTimeout` are in seconds.
- Addresses in arguments may be given in base58 (`T...`) to tronweb and TronBox; inside the contract they are 20 bytes.

## Settling a dispute with TruceHQ

1. Either party calls `dispute(dealId)` before the release deadline.
2. The two agents open an Agent Truce at `https://trucehq.com/mcp` (or `POST https://trucehq.com/api/agent-disputes`), passing the escrow deal as a reference, e.g. `refs.order_id = "tron:<chainId>:<escrow base58>:<dealId>"`.
3. When both agree, TruceHQ returns the agreement and its `agreement_digest`.
4. Each party signs the split as shown above, using the SHA-256 `agreement_digest` as the `bytes32` value (drop the `sha256:` prefix and add `0x`).
5. Anyone submits both signatures to `settle(...)`. The `Settled` event records the digest.

## Deploying (not done; for reference)

Deploy to Nile testnet first and run the full flow with a test token before mainnet.

**With TronBox:**

```
npm install -g tronbox
tronbox init
```

Copy `contracts/TruceEscrowTron.sol` into the project. In `tronbox.js`, add networks with `fullHost` (`https://nile.trongrid.io` for Nile, `https://api.trongrid.io` for mainnet), the deployer key from an environment variable (never commit it), a `feeLimit`, and a compiler version of 0.8.20 or later that TronBox offers. TronBox uses Tron's own build of solc. A migration:

```js
const TruceEscrowTron = artifacts.require('TruceEscrowTron');
module.exports = function (deployer) {
  deployer.deploy(TruceEscrowTron,
    ['T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb', '<TRC-20 address>'],
    [100000000, 100000000]);
};
```

Then `tronbox migrate --network nile`. Check which Solidity versions and EVM target TronBox supports at the time; the tests here compile with standard solc 0.8.28 for the `paris` target, which is not the same compiler.

**With TronLink:** open a contract deployment tool that connects to TronLink (for example the deploy page on Tronscan, or TronIDE), switch TronLink to Nile, paste or upload `TruceEscrowTron.sol`, compile with a 0.8.20+ compiler, enter the two constructor arrays, set the fee limit and confirm in TronLink.

After deploying, call `eip712Domain()` and `maxAmount(token)` to check the chain id and caps, and verify the source on Tronscan.

## Tests

```
cd tron-escrow
npm install
npm test
```

Runs on ganache (EVM) with standard solc. ganache is close enough to check the contract's logic, but it is not the TVM. Not covered: energy and bandwidth, TRC-10, account activation, Tron's own solc build, the real chain id on Tron, and the behaviour of the real USDT contract on mainnet.

Covered: release, claim after deadline, dispute blocking claim, signed split with TIP-712 signatures made by tronweb (wrong signer, swapped roles, wrong chain id and bad split rejected), no replay, even split on dispute timeout, seller refund, constructor validation and caps (over cap and unlisted coin rejected, exactly the cap accepted), and open plus signed split with five token types: standard, no return value (Ethereum USDT-style), returns `false` on success (USDT-on-Tron-style), fee-on-transfer, and one that returns `true` but moves nothing (caught by the balance check, funds stay locked and pay out once the token works again).

## Not verified

- That USDT on Tron returns `false` on a successful `transfer`, and how its `transferFrom` behaves. The contract is written to work either way.
- The chain ids above, what `block.chainid` returns on the current TVM, and whether TronLink accepts that value in a TIP-712 domain.
- Which Solidity versions TronBox and Tronscan currently offer, and whether this contract compiles unchanged with them.
- That `block.timestamp` is in seconds on the TVM.
- Energy costs of each function and of USDT transfers.

Questions: frank@trucehq.com · https://trucehq.com/agents
