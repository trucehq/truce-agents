// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;
// Test-only tokens.
//   Std:       standard TRC-20, returns true (USDC-style).
//   NoReturn:  returns nothing from transfer/transferFrom (Ethereum USDT-style).
//   UsdtTron:  declares `returns (bool)` but returns false even when the transfer succeeded, the
//              behaviour reported for USDT on Tron. A return-value check would treat every payout as failed.
//   Fee:       takes a 1% fee from the amount received.
//   Liar:      returns true but, once `broken` is set, moves nothing. Only a balance check catches it.
contract Std { mapping(address=>uint) public balanceOf; mapping(address=>mapping(address=>uint)) public allowance;
  function mint(address a,uint v) external { balanceOf[a]+=v; }
  function approve(address s,uint v) external returns(bool){ allowance[msg.sender][s]=v; return true; }
  function _move(address f,address t,uint v) internal virtual { require(balanceOf[f]>=v); balanceOf[f]-=v; balanceOf[t]+=v; }
  function transfer(address t,uint v) external virtual returns(bool){ _move(msg.sender,t,v); return true; }
  function transferFrom(address f,address t,uint v) external virtual returns(bool){ require(allowance[f][msg.sender]>=v); allowance[f][msg.sender]-=v; _move(f,t,v); return true; } }
contract NoReturn { mapping(address=>uint) public balanceOf; mapping(address=>mapping(address=>uint)) public allowance;
  function mint(address a,uint v) external { balanceOf[a]+=v; }
  function approve(address s,uint v) external { allowance[msg.sender][s]=v; }
  function transfer(address t,uint v) external { require(balanceOf[msg.sender]>=v); balanceOf[msg.sender]-=v; balanceOf[t]+=v; }
  function transferFrom(address f,address t,uint v) external { require(allowance[f][msg.sender]>=v && balanceOf[f]>=v); allowance[f][msg.sender]-=v; balanceOf[f]-=v; balanceOf[t]+=v; } }
contract UsdtTron is Std {
  function transfer(address t,uint v) external override returns(bool ok){ _move(msg.sender,t,v); ok=false; }
  function transferFrom(address f,address t,uint v) external override returns(bool ok){ require(allowance[f][msg.sender]>=v); allowance[f][msg.sender]-=v; _move(f,t,v); ok=false; } }
contract Fee is Std { function _move(address f,address t,uint v) internal override { require(balanceOf[f]>=v); balanceOf[f]-=v; balanceOf[t]+=v-v/100; } }
contract Liar is Std { bool public broken;
  function setBroken(bool b) external { broken=b; }
  function _move(address f,address t,uint v) internal override { if (broken) return; super._move(f,t,v); } }
