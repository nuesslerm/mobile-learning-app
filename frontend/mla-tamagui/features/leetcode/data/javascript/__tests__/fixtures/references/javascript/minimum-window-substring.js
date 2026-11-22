var minWindow = function(s, t) {
  const needed = new Map();
  for (const c of t) needed.set(c, (needed.get(c) || 0) + 1);
  let missing = t.length;
  let bestStart = 0;
  let bestLength = Infinity;
  let start = 0;
  for (let end = 0; end < s.length; end += 1) {
    const entering = s[end];
    if ((needed.get(entering) || 0) > 0) missing -= 1;
    needed.set(entering, (needed.get(entering) || 0) - 1);
    while (missing === 0) {
      if (end - start + 1 < bestLength) {
        bestStart = start;
        bestLength = end - start + 1;
      }
      const leaving = s[start];
      needed.set(leaving, needed.get(leaving) + 1);
      if (needed.get(leaving) > 0) missing += 1;
      start += 1;
    }
  }
  return bestLength === Infinity ? '' : s.slice(bestStart, bestStart + bestLength);
};
