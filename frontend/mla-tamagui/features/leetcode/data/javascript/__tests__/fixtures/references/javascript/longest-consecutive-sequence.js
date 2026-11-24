var longestConsecutive = function(nums) {
  const present = new Set(nums);
  let best = 0;
  for (const n of present) {
    if (present.has(n - 1)) continue;
    let end = n;
    while (present.has(end + 1)) end += 1;
    best = Math.max(best, end - n + 1);
  }
  return best;
};
