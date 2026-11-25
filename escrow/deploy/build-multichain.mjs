import solc from 'solc'; import fs from 'fs'; import { ethers } from 'ethers';
const src = fs.readFileSync('contracts/TruceEscrow.sol','utf8');
const out = JSON.parse(solc.compile(JSON.stringify({language:'Solidity',sources:{'TruceEscrow.sol':{content:src}},settings:{optimizer:{enabled:true,runs:200},evmVersion:'cancun',outputSelection:{'*':{'*':['abi','evm.bytecode.object']}}}})));
const bc = out.contracts['TruceEscrow.sol'].TruceEscrow.evm.bytecode.object;
const E=ethers.parseEther, U=1000n*10n**6n, Z=ethers.ZeroAddress;
// USDC addresses from Circle's official list; native caps ~ $1,000 or less at recent prices
const chains = {
  base:      {chainId:'0x2105', tokens:[[Z,E('0.4')],['0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913',U],['0x60a3e35cc302bfa44cb288bc5a4f316fdb1adb42',U]]},
  arbitrum:  {chainId:'0xa4b1', tokens:[[Z,E('0.4')],['0xaf88d065e77c8cC2239327C5EDb3A432268e5831',U]]},
  avalanche: {chainId:'0xa86a', tokens:[[Z,E('120')],['0xB97EF9Ef8734C71904D8002F8b6Bc66Dd9c48a6E',U]]},
  polygon:   {chainId:'0x89',   tokens:[[Z,E('9000')],['0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359',U]]},
  bnb:       {chainId:'0x38',   tokens:[[Z,E('1.3')]]},
};
const res={};
for (const [k,c] of Object.entries(chains)) {
  const t=c.tokens.map(x=>ethers.getAddress(x[0].toLowerCase())), caps=c.tokens.map(x=>x[1]);
  res[k]={chainId:c.chainId, tokens:t, caps:caps.map(String), data:'0x'+bc+ethers.AbiCoder.defaultAbiCoder().encode(['address[]','uint256[]'],[t,caps]).slice(2)};
}
fs.writeFileSync('multichain-deploy.json', JSON.stringify(res,null,1));
for (const [k,v] of Object.entries(res)) console.log(k, v.chainId, v.tokens.length, v.data.length);
