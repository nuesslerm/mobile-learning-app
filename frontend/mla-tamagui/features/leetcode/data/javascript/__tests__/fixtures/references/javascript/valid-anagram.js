var isAnagram = function(s, t) {
  if (s.length !== t.length) return false;
  const counts = new Map();
  for (const c of s) counts.set(c, (counts.get(c) || 0) + 1);
  for (const c of t) {
    const left = counts.get(c) || 0;
    if (left === 0) return false;
    counts.set(c, left - 1);
  }
  return true;
};
