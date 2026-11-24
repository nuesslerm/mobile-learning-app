var lengthOfLongestSubstring = function(s) {
  const lastSeen = new Map();
  let start = 0;
  let best = 0;
  for (let i = 0; i < s.length; i += 1) {
    if (lastSeen.get(s[i]) >= start) start = lastSeen.get(s[i]) + 1;
    lastSeen.set(s[i], i);
    best = Math.max(best, i - start + 1);
  }
  return best;
};
