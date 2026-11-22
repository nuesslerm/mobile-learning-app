var combinationSum4 = function(nums, target) {
  const ways = new Array(target + 1).fill(0);
  ways[0] = 1;
  for (let total = 1; total <= target; total += 1) {
    for (const n of nums) {
      if (n <= total) ways[total] += ways[total - n];
    }
  }
  return ways[target];
};
