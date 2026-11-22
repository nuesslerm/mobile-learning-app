var characterReplacement = function(s, k) {
  const counts = new Map();
  let start = 0;
  let mostCommon = 0;
  let best = 0;
  for (let end = 0; end < s.length; end += 1) {
    const count = (counts.get(s[end]) || 0) + 1;
    counts.set(s[end], count);
    // mostCommon is never lowered: a longer window needs a higher count than any seen before
    mostCommon = Math.max(mostCommon, count);
    if (end - start + 1 - mostCommon > k) {
      counts.set(s[start], counts.get(s[start]) - 1);
      start += 1;
    }
    best = Math.max(best, end - start + 1);
  }
  return best;
};
