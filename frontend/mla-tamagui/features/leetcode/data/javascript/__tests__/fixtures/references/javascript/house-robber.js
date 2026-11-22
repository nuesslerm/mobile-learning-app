var rob = function(nums) {
  let best = 0;
  let beforeThat = 0;
  for (const money of nums) {
    [beforeThat, best] = [best, Math.max(best, beforeThat + money)];
  }
  return best;
};
