var maxProduct = function(nums) {
  let highest = nums[0];
  let lowest = nums[0];
  let best = nums[0];
  for (let i = 1; i < nums.length; i += 1) {
    const candidates = [nums[i], highest * nums[i], lowest * nums[i]];
    highest = Math.max(...candidates);
    lowest = Math.min(...candidates);
    best = Math.max(best, highest);
  }
  return best;
};
