function robLine(houses) {
  let best = 0;
  let beforeThat = 0;
  for (const money of houses) {
    [beforeThat, best] = [best, Math.max(best, beforeThat + money)];
  }
  return best;
}

var rob = function(nums) {
  if (nums.length === 1) return nums[0];
  return Math.max(robLine(nums.slice(0, -1)), robLine(nums.slice(1)));
};
