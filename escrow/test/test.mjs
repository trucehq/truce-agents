// Local tests for TruceEscrow. Keys below are ganache's public, deterministic TEST keys: never use them on a real network.
import solc from 'solc'; import fs from 'fs'; import ganache from 'ganache'; import { ethers } from 'ethers'; import assert from 'assert';
const src=fs.readFileSync(new URL('../contracts/TruceEscrow.sol', import.meta.url),'utf8');
const out=JSON.parse(solc.compile(JSON.stringify({language:'Solidity',sources:{'E.sol':{content:src}},settings:{evmVersion:'paris',optimizer:{enabled:true,runs:200},outputSelection:{'*':{'*':['abi','evm.bytecode.object']}}}})));
if(out.errors) for(const e of out.errors) console.log(e.severity, e.formattedMessage.split('\n')[0]);
const c=out.contracts['E.sol'].TruceEscrow;
const g=ganache.provider({logging:{quiet:true},wallet:{totalAccounts:3,deterministic:true},chain:{chainId:1337}});
const p=new ethers.BrowserProvider(g,undefined,{cacheTimeout:-1}); const [buyer,seller,other]=await Promise.all([0,1,2].map(i=>p.getSigner(i)));
const E=await new ethers.ContractFactory(c.abi,c.evm.bytecode.object,buyer).deploy(); await E.waitForDeployment();
const now=async()=>(await p.getBlock('latest')).timestamp;
const jump=async s=>{await p.send('evm_increaseTime',[s]); await p.send('evm_mine',[]);};
const bal=a=>p.getBalance(a);
const ONE=ethers.parseEther('1'), DAY=86400;
const open=async()=>{const t=await (await E.connect(buyer).open(seller.address,ethers.ZeroAddress,ONE,(await now())+DAY,DAY,{value:ONE})).wait(); return E.interface.parseLog(t.logs[0]).args.id;};
// 1 happy release
let id=await open(); let s0=await bal(seller.address); await (await E.connect(buyer).release(id)).wait(); assert.equal(await bal(seller.address)-s0,ONE); console.log('ok release');
// 2 claim after deadline
id=await open(); await assert.rejects(E.connect(seller).claim(id)); await jump(DAY+1); s0=await bal(seller.address); const tx=await (await E.connect(seller).claim(id)).wait(); const gas=tx.gasUsed*tx.gasPrice; assert.equal(await bal(seller.address)-s0+gas,ONE); console.log('ok claim');
// 3 dispute blocks claim, signed settlement splits
id=await open(); await (await E.connect(buyer).dispute(id)).wait(); await jump(DAY+1); await assert.rejects(E.connect(seller).claim(id));
const net=await p.getNetwork(); const domain={name:'TruceEscrow',version:'1',chainId:net.chainId,verifyingContract:await E.getAddress()};
const types={Settle:[{name:'dealId',type:'uint256'},{name:'toSeller',type:'uint256'},{name:'toBuyer',type:'uint256'},{name:'agreementDigest',type:'bytes32'}]};
const dg=ethers.keccak256(ethers.toUtf8Bytes('sha256:abc'));
const v={dealId:id,toSeller:ethers.parseEther('0.6'),toBuyer:ethers.parseEther('0.4'),agreementDigest:dg};
const K=['0x4f3edf983ac636a65a842ce7c78d9aa706d3b113bce9c46f30d7d21715b23b1d','0x6cbed15c793ce57650b9877cf6fa156fbef513c4e6134f022a85b1ffdd59b2a1','0x6370fd033278c143179d81c5526140625662b8daa446c22ee2d73db3707e620c'].map(k=>new ethers.Wallet(k));
assert.equal(K[0].address,buyer.address); assert.equal(K[1].address,seller.address);
const sb=await K[0].signTypedData(domain,types,v), ss=await K[1].signTypedData(domain,types,v);
// wrong signer rejected
const so=await K[2].signTypedData(domain,types,v); await assert.rejects(E.connect(other).settle(id,v.toSeller,v.toBuyer,dg,sb,so));
// bad split rejected
await assert.rejects(E.connect(other).settle(id,v.toSeller,v.toSeller,dg,sb,ss));
s0=await bal(seller.address); const b0=await bal(buyer.address);
await (await E.connect(other).settle(id,v.toSeller,v.toBuyer,dg,sb,ss)).wait();
assert.equal(await bal(seller.address)-s0,v.toSeller); assert.equal(await bal(buyer.address)-b0,v.toBuyer); console.log('ok settle');
// replay rejected
await assert.rejects(E.connect(other).settle(id,v.toSeller,v.toBuyer,dg,sb,ss)); console.log('ok no replay');
// 4 dispute timeout splits evenly
id=await open(); await (await E.connect(seller).dispute(id)).wait(); await assert.rejects(E.connect(other).expire(id)); await jump(DAY+1);
const b1=await bal(buyer.address), s1=await bal(seller.address); await (await E.connect(other).expire(id)).wait(); assert.equal(await bal(buyer.address)-b1,ONE/2n); assert.equal(await bal(seller.address)-s1,ONE/2n); console.log('ok expire (even split)');
// 5 non-party cannot dispute/release; seller refund works
id=await open(); await assert.rejects(E.connect(other).dispute(id)); await assert.rejects(E.connect(other).release(id));
const b2=await bal(buyer.address); await (await E.connect(seller).refund(id)).wait(); assert.equal(await bal(buyer.address)-b2,ONE); console.log('ok refund');
// 6 ERC-20s: standard, no-return (USDT-style) and fee-on-transfer, through open + settle
const msrc=fs.readFileSync(new URL('./mocks/Tokens.sol', import.meta.url),'utf8');
const mo=JSON.parse(solc.compile(JSON.stringify({language:'Solidity',sources:{'T.sol':{content:msrc}},settings:{evmVersion:'paris',outputSelection:{'*':{'*':['abi','evm.bytecode.object']}}}})));
const T=1000000n;
for (const name of ['Std','NoReturn','Fee']) {
  const m=mo.contracts['T.sol'][name]; const tok=await new ethers.ContractFactory(m.abi,m.evm.bytecode.object,buyer).deploy(); await tok.waitForDeployment();
  await (await tok.mint(buyer.address,T)).wait(); await (await tok.approve(await E.getAddress(),T)).wait();
  const r=await (await E.connect(buyer).open(seller.address,await tok.getAddress(),T,(await now())+DAY,DAY)).wait();
  const tid=E.interface.parseLog(r.logs.find(l=>l.address===E.target)).args.id; const held=(await E.deals(tid)).amount;
  assert.equal(held, name==='Fee'? T-T/100n : T);
  const toS=held/4n, toB=held-toS, dg=ethers.id('agreement');
  const dom={name:'TruceEscrow',version:'1',chainId:1337,verifyingContract:await E.getAddress()};
  const ty={Settle:[{name:'dealId',type:'uint256'},{name:'toSeller',type:'uint256'},{name:'toBuyer',type:'uint256'},{name:'agreementDigest',type:'bytes32'}]};
  const v={dealId:tid,toSeller:toS,toBuyer:toB,agreementDigest:dg};
  const s1=await tok.balanceOf(seller.address);
  await (await E.connect(other).settle(tid,toS,toB,dg,await K[0].signTypedData(dom,ty,v),await K[1].signTypedData(dom,ty,v))).wait();
  const got=(await tok.balanceOf(seller.address))-s1; assert.equal(got, name==='Fee'? toS-toS/100n : toS);
  console.log('ok token '+name);
}
console.log('ALL PASS');
