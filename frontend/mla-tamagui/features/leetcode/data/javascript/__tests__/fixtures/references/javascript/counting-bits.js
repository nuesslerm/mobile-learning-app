var countBits = function(n) {
  const bits = new Array(n + 1).fill(0);
  for (let i = 1; i <= n; i += 1) bits[i] = bits[i >> 1] + (i & 1);
  return bits;
};
