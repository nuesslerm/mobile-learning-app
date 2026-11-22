var reverseBits = function(n) {
  let reversed = 0;
  for (let i = 0; i < 32; i += 1) {
    reversed = (reversed << 1) | (n & 1);
    n >>>= 1;
  }
  return reversed >>> 0;
};
