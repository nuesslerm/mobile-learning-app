var wordBreak = function(s, wordDict) {
  const words = new Set(wordDict);
  const reachable = new Array(s.length + 1).fill(false);
  reachable[0] = true;
  for (let end = 1; end <= s.length; end += 1) {
    for (let start = 0; start < end; start += 1) {
      if (reachable[start] && words.has(s.slice(start, end))) {
        reachable[end] = true;
        break;
      }
    }
  }
  return reachable[s.length];
};
