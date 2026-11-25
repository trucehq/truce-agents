// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
// Test-only tokens: a standard one (USDC-style), one that returns nothing (USDT-style), one that takes a 1% fee.
contract Std { mapping(address=>uint) public balanceOf; mapping(address=>mapping(address=>uint)) public allowance;
  function mint(address a,uint v) external { balanceOf[a]+=v; }
  function approve(address s,uint v) external returns(bool){ allowance[msg.sender][s]=v; return true; }
  function _move(address f,address t,uint v) internal virtual { require(balanceOf[f]>=v); balanceOf[f]-=v; balanceOf[t]+=v; }
  function transfer(address t,uint v) external returns(bool){ _move(msg.sender,t,v); return true; }
  function transferFrom(address f,address t,uint v) external returns(bool){ require(allowance[f][msg.sender]>=v); allowance[f][msg.sender]-=v; _move(f,t,v); return true; } }
contract NoReturn { mapping(address=>uint) public balanceOf; mapping(address=>mapping(address=>uint)) public allowance;
  function mint(address a,uint v) external { balanceOf[a]+=v; }
  function approve(address s,uint v) external { allowance[msg.sender][s]=v; }
  function transfer(address t,uint v) external { require(balanceOf[msg.sender]>=v); balanceOf[msg.sender]-=v; balanceOf[t]+=v; }
  function transferFrom(address f,address t,uint v) external { require(allowance[f][msg.sender]>=v && balanceOf[f]>=v); allowance[f][msg.sender]-=v; balanceOf[f]-=v; balanceOf[t]+=v; } }
contract Fee is Std { function _move(address f,address t,uint v) internal override { require(balanceOf[f]>=v); balanceOf[f]-=v; balanceOf[t]+=v-v/100; } }
