// Logic tests for TruceEscrowTron on ganache (EVM). ganache is not the TVM: energy, bandwidth,
// TRC-10, account activation and the behaviour of real TRC-20 contracts on Tron mainnet are NOT covered.
// Settle signatures are produced with tronweb's TIP-712 signer (Trx._signTypedData) using Tron base58
// addresses, to check that its hashing matches what the contract verifies.
import solc from 'solc'; import fs from 'fs'; import ganache from 'ganache'; import { ethers } from 'ethers'; import assert from 'assert';
import { TronWeb, Trx } from 'tronweb';

const compile = (file, name) => {
  const out = JSON.parse(solc.compile(JSON.stringify({ language: 'Solidity', sources: { [name]: { content: fs.readFileSync(new URL(file, import.meta.url), 'utf8') } },
    settings: { evmVersion: 'paris', optimizer: { enabled: true, runs: 200 }, outputSelection: { '*': { '*': ['abi', 'evm.bytecode.object'] } } } })));
  for (const e of out.errors || []) { console.log(e.severity, e.formattedMessage.split('\n')[0]); if (e.severity === 'error') process.exit(1); }
  return out.contracts[name];
};
const c = compile('../contracts/TruceEscrowTron.sol', 'E.sol').TruceEscrowTron;
const mo = compile('./mocks/Tokens.sol', 'T.sol');

const g = ganache.provider({ logging: { quiet: true }, wallet: { totalAccounts: 3, deterministic: true }, chain: { chainId: 1337 } });
const p = new ethers.BrowserProvider(g, undefined, { cacheTimeout: -1 });
const [buyer, seller, other] = await Promise.all([0, 1, 2].map(i => p.getSigner(i)));
const KEYS = ['4f3edf983ac636a65a842ce7c78d9aa706d3b113bce9c46f30d7d21715b23b1d', '6cbed15c793ce57650b9877cf6fa156fbef513c4e6134f022a85b1ffdd59b2a1', '6370fd033278c143179d81c5526140625662b8daa446c22ee2d73db3707e620c'];
assert.equal(new ethers.Wallet(KEYS[0]).address, buyer.address); assert.equal(new ethers.Wallet(KEYS[1]).address, seller.address);
const toTron = a => TronWeb.address.fromHex('41' + a.slice(2)); // 0x... (20 bytes) -> T... base58
assert.equal(TronWeb.address.fromPrivateKey(KEYS[0]), toTron(buyer.address)); // same key, same 20 bytes on both chains

const SUN = 1_000_000n; // 1 TRX = 1e6 sun (ganache's native coin stands in for TRX)
const ONE = SUN, DAY = 86400;
const deploy = async (tokens, caps) => { const E = await new ethers.ContractFactory(c.abi, c.evm.bytecode.object, buyer).deploy(tokens, caps); await E.waitForDeployment(); return E; };
const E = await deploy([ethers.ZeroAddress], [5n * SUN]);
await assert.rejects(deploy([], [])); await assert.rejects(deploy([ethers.ZeroAddress], [0])); await assert.rejects(deploy([ethers.ZeroAddress], [1, 2]));
const now = async () => (await p.getBlock('latest')).timestamp;
const jump = async s => { await p.send('evm_increaseTime', [s]); await p.send('evm_mine', []); };
const bal = a => p.getBalance(a);
const open = async () => { const t = await (await E.connect(buyer).open(seller.address, ethers.ZeroAddress, ONE, (await now()) + DAY, DAY, { value: ONE })).wait(); return E.interface.parseLog(t.logs[0]).args.id; };

// TIP-712 signing as a Tron wallet would do it: base58 verifyingContract, chain id read from the contract.
const types = { Settle: [{ name: 'dealId', type: 'uint256' }, { name: 'toSeller', type: 'uint256' }, { name: 'toBuyer', type: 'uint256' }, { name: 'agreementDigest', type: 'bytes32' }] };
const domainOf = async (Ec) => { const d = await Ec.eip712Domain(); return { name: d.name, version: d.version, chainId: d.chainId.toString(), verifyingContract: toTron(d.verifyingContract) }; };
const tronSign = (domain, v, key) => Trx._signTypedData(domain, types, { dealId: v.dealId.toString(), toSeller: v.toSeller.toString(), toBuyer: v.toBuyer.toString(), agreementDigest: v.agreementDigest }, key);

// 1 happy release
let id = await open(); let s0 = await bal(seller.address); await (await E.connect(buyer).release(id)).wait(); assert.equal(await bal(seller.address) - s0, ONE); console.log('ok release');
// 2 claim after deadline
id = await open(); await assert.rejects(E.connect(seller).claim(id)); await jump(DAY + 1); s0 = await bal(seller.address);
const tx = await (await E.connect(seller).claim(id)).wait(); assert.equal(await bal(seller.address) - s0 + tx.gasUsed * tx.gasPrice, ONE); console.log('ok claim');
// 3 dispute blocks claim, signed settlement splits
id = await open(); await (await E.connect(buyer).dispute(id)).wait(); await jump(DAY + 1); await assert.rejects(E.connect(seller).claim(id));
const domain = await domainOf(E); assert.equal(domain.chainId, '1337'); assert.equal(await E.DOMAIN_CHAIN_ID(), 1337n);
const dg = ethers.keccak256(ethers.toUtf8Bytes('sha256:abc'));
const v = { dealId: id, toSeller: 600_000n, toBuyer: 400_000n, agreementDigest: dg };
const sb = tronSign(domain, v, KEYS[0]), ss = tronSign(domain, v, KEYS[1]);
// tronweb (base58 addresses) and ethers (hex addresses) must produce the same signature
assert.equal(sb, await new ethers.Wallet(KEYS[0]).signTypedData({ ...domain, verifyingContract: await E.getAddress() }, types, v));
await assert.rejects(E.connect(other).settle(id, v.toSeller, v.toBuyer, dg, sb, tronSign(domain, v, KEYS[2]))); // wrong signer
await assert.rejects(E.connect(other).settle(id, v.toSeller, v.toBuyer, dg, ss, sb)); // swapped roles
const mainnetDomain = { ...domain, chainId: '728126428' }; // signed for another chain id: must fail here
await assert.rejects(E.connect(other).settle(id, v.toSeller, v.toBuyer, dg, tronSign(mainnetDomain, v, KEYS[0]), tronSign(mainnetDomain, v, KEYS[1])));
await assert.rejects(E.connect(other).settle(id, v.toSeller, v.toSeller, dg, sb, ss)); // bad split
s0 = await bal(seller.address); const b0 = await bal(buyer.address);
await (await E.connect(other).settle(id, v.toSeller, v.toBuyer, dg, sb, ss)).wait();
assert.equal(await bal(seller.address) - s0, v.toSeller); assert.equal(await bal(buyer.address) - b0, v.toBuyer); console.log('ok settle (TIP-712 signatures from tronweb)');
// replay rejected
await assert.rejects(E.connect(other).settle(id, v.toSeller, v.toBuyer, dg, sb, ss)); console.log('ok no replay');
// 4 dispute timeout splits evenly
id = await open(); await (await E.connect(seller).dispute(id)).wait(); await assert.rejects(E.connect(other).expire(id)); await jump(DAY + 1);
const b1 = await bal(buyer.address), s1 = await bal(seller.address); await (await E.connect(other).expire(id)).wait();
assert.equal(await bal(buyer.address) - b1, ONE / 2n); assert.equal(await bal(seller.address) - s1, ONE / 2n); console.log('ok expire (even split)');
// 5 non-party cannot dispute/release; seller refund works
id = await open(); await assert.rejects(E.connect(other).dispute(id)); await assert.rejects(E.connect(other).release(id));
const b2 = await bal(buyer.address); await (await E.connect(seller).refund(id)).wait(); assert.equal(await bal(buyer.address) - b2, ONE); console.log('ok refund');
// 6 caps: TRX over the cap refused, exactly the cap accepted, unlisted token refused (below)
await assert.rejects(E.connect(buyer).open(seller.address, ethers.ZeroAddress, 6n * SUN, (await now()) + DAY, DAY, { value: 6n * SUN }), /NotAccepted|revert/);
await (await E.connect(buyer).open(seller.address, ethers.ZeroAddress, 5n * SUN, (await now()) + DAY, DAY, { value: 5n * SUN })).wait();
console.log('ok cap');
// 7 TRC-20s through open + settle: standard, no return value, USDT-Tron-style (returns false on success), fee-on-transfer
const T = 1_000_000n; // 1 USDT (6 decimals)
const newToken = async name => { const m = mo[name]; const t = await new ethers.ContractFactory(m.abi, m.evm.bytecode.object, buyer).deploy(); await t.waitForDeployment(); await (await t.mint(buyer.address, 10n * T)).wait(); return t; };
for (const name of ['Std', 'NoReturn', 'UsdtTron', 'Fee']) {
  const tok = await newToken(name); const ta = await tok.getAddress();
  await (await tok.approve(await E.getAddress(), T)).wait();
  await assert.rejects(E.connect(buyer).open(seller.address, ta, T, (await now()) + DAY, DAY)); // not listed on E
  const E2 = await deploy([ta], [T]); await (await tok.approve(await E2.getAddress(), 10n * T)).wait();
  await assert.rejects(E2.connect(buyer).open(seller.address, ta, T + 1n, (await now()) + DAY, DAY)); // over cap
  const r = await (await E2.connect(buyer).open(seller.address, ta, T, (await now()) + DAY, DAY)).wait();
  const tid = E2.interface.parseLog(r.logs.find(l => l.address === E2.target)).args.id; const held = (await E2.deals(tid)).amount;
  assert.equal(held, name === 'Fee' ? T - T / 100n : T);
  const toS = held / 4n, toB = held - toS, adg = ethers.id('agreement');
  const dom = await domainOf(E2); const vv = { dealId: tid, toSeller: toS, toBuyer: toB, agreementDigest: adg };
  const sS0 = await tok.balanceOf(seller.address), sB0 = await tok.balanceOf(buyer.address);
  await (await E2.connect(other).settle(tid, toS, toB, adg, tronSign(dom, vv, KEYS[0]), tronSign(dom, vv, KEYS[1]))).wait();
  assert.equal((await tok.balanceOf(seller.address)) - sS0, name === 'Fee' ? toS - toS / 100n : toS);
  assert.equal((await tok.balanceOf(buyer.address)) - sB0, name === 'Fee' ? toB - toB / 100n : toB);
  assert.equal(await tok.balanceOf(await E2.getAddress()), 0n);
  console.log('ok token ' + name);
}
// 8 a token that reports success but moves nothing: open refused, payout refused (funds stay locked, not lost)
{
  const tok = await newToken('Liar'); const ta = await tok.getAddress(); const E3 = await deploy([ta], [T]);
  await (await tok.approve(await E3.getAddress(), 10n * T)).wait();
  await (await tok.setBroken(true)).wait();
  await assert.rejects(E3.connect(buyer).open(seller.address, ta, T, (await now()) + DAY, DAY));
  await (await tok.setBroken(false)).wait();
  const r = await (await E3.connect(buyer).open(seller.address, ta, T, (await now()) + DAY, DAY)).wait();
  const tid = E3.interface.parseLog(r.logs.find(l => l.address === E3.target)).args.id;
  await (await tok.setBroken(true)).wait();
  await assert.rejects(E3.connect(buyer).release(tid));
  assert.equal((await E3.deals(tid)).status, 1n); // still Funded
  await (await tok.setBroken(false)).wait();
  await (await E3.connect(buyer).release(tid)).wait(); assert.equal(await tok.balanceOf(seller.address), T);
  console.log('ok token Liar (balance check catches a false success)');
}
console.log('ALL PASS');
