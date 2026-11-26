// TruceEscrow on Arc testnet, driven by Circle developer-controlled wallets.
// Two Circle wallets (buyer agent, seller agent) open a USDC deal, dispute it,
// sign the same Truce settlement (EIP-712) through Circle, and the contract pays out.
//
// Needs two secrets in the environment, never in code or logs:
//   CIRCLE_API_KEY        Circle Console > Keys (testnet)
//   CIRCLE_ENTITY_SECRET  Circle Console > Configurator > Entity Secret
// Run: node circle/arc-wallets-demo.mjs
import { initiateDeveloperControlledWalletsClient } from '@circle-fin/developer-controlled-wallets';
import { Interface, keccak256, toUtf8Bytes } from 'ethers';

const CHAIN = 'ARC-TESTNET';
const CHAIN_ID = 5042002;
const RPC = 'https://rpc.testnet.arc.io';
const EXPLORER = 'https://explorer.testnet.arc.io';
const ESCROW = '0x5c2e0e4a9be3ae94d31de2f16ded89d62babc17f'; // TruceEscrow, verified on Arc testnet
const USDC = '0x3600000000000000000000000000000000000000'; // Circle USDC on Arc testnet (6 decimals)
const AMOUNT = 2_000_000n; // 2 USDC
const TO_SELLER = 1_200_000n; // the Truce outcome in this demo: 1.2 USDC to the seller
const TO_BUYER = AMOUNT - TO_SELLER; // 0.8 USDC back to the buyer

for (const k of ['CIRCLE_API_KEY', 'CIRCLE_ENTITY_SECRET']) {
  if (!process.env[k]) throw new Error(`Missing ${k}`);
}
let circle;
const esc = new Interface(['function nextId() view returns (uint256)']);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);

async function rpc(method, params) {
  const r = await fetch(RPC, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }) });
  const j = await r.json();
  if (j.error) throw new Error(JSON.stringify(j.error));
  return j.result;
}
const usdcBalance = async (addr) =>
  BigInt(await rpc('eth_call', [{ to: USDC, data: '0x70a08231' + addr.slice(2).toLowerCase().padStart(64, '0') }, 'latest']));

async function wallets() {
  const name = 'TruceHQ agents (Arc testnet demo)';
  const sets = (await circle.listWalletSets({})).data?.walletSets ?? [];
  let set = sets.find((s) => s.name === name);
  if (!set) set = (await circle.createWalletSet({ name })).data.walletSet;
  let list = (await circle.listWallets({ walletSetId: set.id, blockchain: CHAIN })).data?.wallets ?? [];
  if (list.length < 2) {
    const made = await circle.createWallets({ walletSetId: set.id, blockchains: [CHAIN], count: 2 - list.length, accountType: 'EOA' });
    list = list.concat(made.data.wallets);
  }
  // The funded wallet acts as the buyer agent; the other is the seller agent.
  const [a, b] = list;
  return (await usdcBalance(a.address)) >= (await usdcBalance(b.address)) ? { buyer: a, seller: b } : { buyer: b, seller: a };
}

async function waitTx(id, label) {
  for (let i = 0; i < 90; i++) {
    const t = (await circle.getTransaction({ id })).data.transaction;
    if (t.state === 'COMPLETE' || t.state === 'CONFIRMED') {
      if (t.state === 'COMPLETE') { log(`${label}: ${EXPLORER}/tx/${t.txHash}`); return t.txHash; }
    }
    if (['FAILED', 'DENIED', 'CANCELLED'].includes(t.state)) throw new Error(`${label} ${t.state}: ${t.errorReason || ''} ${t.errorDetails || ''}`);
    await sleep(2000);
  }
  throw new Error(`${label}: timed out`);
}

async function call(wallet, label, abiFunctionSignature, abiParameters) {
  const r = await circle.createContractExecutionTransaction({
    walletId: wallet.id,
    contractAddress: abiFunctionSignature.startsWith('approve') ? USDC : ESCROW,
    abiFunctionSignature,
    abiParameters,
    fee: { type: 'level', config: { feeLevel: 'MEDIUM' } },
  });
  return waitTx(r.data.id, label);
}

async function signSettle(wallet, message) {
  const typed = {
    types: {
      EIP712Domain: [
        { name: 'name', type: 'string' }, { name: 'version', type: 'string' },
        { name: 'chainId', type: 'uint256' }, { name: 'verifyingContract', type: 'address' },
      ],
      Settle: [
        { name: 'dealId', type: 'uint256' }, { name: 'toSeller', type: 'uint256' },
        { name: 'toBuyer', type: 'uint256' }, { name: 'agreementDigest', type: 'bytes32' },
      ],
    },
    primaryType: 'Settle',
    domain: { name: 'TruceEscrow', version: '1', chainId: CHAIN_ID, verifyingContract: ESCROW },
    message,
  };
  const r = await circle.signTypedData({ walletId: wallet.id, data: JSON.stringify(typed), memo: 'TruceHQ settlement' });
  return r.data.signature;
}

async function main() {
  circle = initiateDeveloperControlledWalletsClient({
    apiKey: process.env.CIRCLE_API_KEY.trim(),
    entitySecret: process.env.CIRCLE_ENTITY_SECRET.trim(),
  });
  const { buyer, seller } = await wallets();
  log(`Buyer agent wallet  (Circle): ${buyer.address}`);
  log(`Seller agent wallet (Circle): ${seller.address}`);

  if ((await usdcBalance(buyer.address)) < AMOUNT + 1_000_000n) {
    throw new Error(`Fund ${buyer.address} with testnet USDC on Arc at https://faucet.circle.com, then run again`);
  }
  log(`Buyer USDC: ${Number(await usdcBalance(buyer.address)) / 1e6}`);

  const dealId = BigInt(esc.decodeFunctionResult('nextId', await rpc('eth_call', [{ to: ESCROW, data: esc.encodeFunctionData('nextId') }, 'latest']))[0]);
  const releaseAt = Math.floor(Date.now() / 1000) + 7200;

  await call(buyer, 'Buyer approves 2 USDC', 'approve(address,uint256)', [ESCROW, AMOUNT.toString()]);
  await call(buyer, `Buyer opens deal #${dealId}`, 'open(address,address,uint256,uint64,uint64)', [seller.address, USDC, AMOUNT.toString(), String(releaseAt), '86400']);
  await call(buyer, `Buyer disputes deal #${dealId}`, 'dispute(uint256)', [dealId.toString()]);

  const agreementDigest = keccak256(toUtf8Bytes(`TruceHQ Arc testnet Circle Wallets demo: deal ${dealId}, seller 1.2 USDC, buyer 0.8 USDC`));
  const message = { dealId: dealId.toString(), toSeller: TO_SELLER.toString(), toBuyer: TO_BUYER.toString(), agreementDigest };
  const sigBuyer = await signSettle(buyer, message);
  const sigSeller = await signSettle(seller, message);
  log('Both Circle wallets signed the same Truce settlement (EIP-712)');

  const sellerBefore = await usdcBalance(seller.address);
  await call(buyer, `Settlement paid out for deal #${dealId}`, 'settle(uint256,uint256,uint256,bytes32,bytes,bytes)', [
    dealId.toString(), TO_SELLER.toString(), TO_BUYER.toString(), agreementDigest, sigBuyer, sigSeller,
  ]);
  log(`Seller received ${Number((await usdcBalance(seller.address)) - sellerBefore) / 1e6} USDC`);
  log('Done.');
}

main().catch((e) => {
  const d = e?.response?.data;
  console.error('FAILED:', e?.message || String(e), d ? JSON.stringify(d) : '');
  process.exit(1);
});
