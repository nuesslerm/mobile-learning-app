var topKFrequent = function(nums, k) {
  const counts = new Map();
  for (const n of nums) counts.set(n, (counts.get(n) || 0) + 1);
  return [...counts.keys()].sort((a, b) => counts.get(b) - counts.get(a)).slice(0, k);
};
