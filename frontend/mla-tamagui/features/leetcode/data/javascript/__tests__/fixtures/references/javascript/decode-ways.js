var numDecodings = function(s) {
  const ways = new Array(s.length + 1).fill(0);
  ways[0] = 1;
  for (let i = 1; i <= s.length; i += 1) {
    if (s[i - 1] !== '0') ways[i] += ways[i - 1];
    if (i >= 2) {
      const pair = Number(s.slice(i - 2, i));
      if (pair >= 10 && pair <= 26) ways[i] += ways[i - 2];
    }
  }
  return ways[s.length];
};
