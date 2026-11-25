import test from "node:test";
import assert from "node:assert/strict";
import { eip55, isEvmAddress, formatUnits, parseUnits, shortDenom } from "../src/tokenUnits.ts";

// the casing QoreChain's testnet uses in its `erc20:0x…` denoms is EIP-55 (checked against the chain's bank metadata)
const CHAIN = [
  "0x02292293c13880a26242690eb70a0e42B8E91c28", // fETH
  "0x4604e7E45b2ecdbfbDc157103f5B2b62cb16451A", // fBTC
  "0x97143B771D34ddb9130b807b135Ae675A5a963CA", // fUSD
  "0x36a4D630898e22dedD8eF5978FC8C527D1698456", // QCAT
  "0xa5b3c3Fb92E2E89555F9909b9004C65F3542eE23", // QWAKK
];

test("EIP-55 reproduces the denoms the chain uses, from any casing", () => {
  for (const a of CHAIN) {
    assert.equal(eip55(a), a);
    assert.equal(eip55(a.toLowerCase()), a);
    assert.equal(eip55(a.toUpperCase().replace("0X", "0x")), a);
  }
  // the EIP-55 reference vector
  assert.equal(eip55("0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed"), "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed");
});

test("EVM addresses are recognised, nothing else is", () => {
  assert.equal(isEvmAddress(CHAIN[0]), true);
  assert.equal(isEvmAddress(CHAIN[0].toLowerCase()), true);
  assert.equal(isEvmAddress("0x1234"), false);
  assert.equal(isEvmAddress("qor1zn2y6duvgcda5htklazv9etu7kwstg0d7ccpuz"), false);
  assert.equal(isEvmAddress(CHAIN[0] + "00"), false);
});

test("amounts by decimals: fUSD 6, fBTC 8, fETH 18", () => {
  assert.equal(formatUnits(1_000_000_000n, 6), "1,000");
  assert.equal(formatUnits(2_000_000n, 8), "0.02");
  assert.equal(formatUnits(500_000_000_000_000_000n, 18), "0.5");
  assert.equal(formatUnits(1n, 18), "0.000000000000000001");
  assert.equal(formatUnits(5n, 0), "5");
  assert.equal(formatUnits(123456789n, 6, 2), "123.45");
  assert.equal(parseUnits("1.5", 6), 1_500_000n);
  assert.equal(parseUnits("0.00000001", 8), 1n);
  assert.equal(parseUnits("0.1", 18), 100_000_000_000_000_000n);
  assert.equal(parseUnits("1000", 6), 1_000_000_000n);
});

test("amounts: too many places, signs, exponents and junk are refused", () => {
  assert.equal(parseUnits("0.0000001", 6), null);
  assert.equal(parseUnits("-1", 6), null);
  assert.equal(parseUnits("1e3", 6), null);
  assert.equal(parseUnits("1,5", 6), null);
  assert.equal(parseUnits("", 6), null);
  assert.equal(parseUnits("1.5", 0), null);
  assert.equal(parseUnits(" 2 ", 0), 2n);
});

test("long denoms are shortened to fit a line", () => {
  assert.equal(shortDenom("erc20:0x97143B771D34ddb9130b807b135Ae675A5a963CA"), "erc20:0x9714..63CA");
  assert.equal(shortDenom("uqor"), "uqor");
});
