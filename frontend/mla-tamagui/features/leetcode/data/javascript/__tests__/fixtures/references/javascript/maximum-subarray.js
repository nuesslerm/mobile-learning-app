var maxSubArray = function(nums) {
  let endingHere = nums[0];
  let best = nums[0];
  for (let i = 1; i < nums.length; i += 1) {
    endingHere = Math.max(nums[i], endingHere + nums[i]);
    best = Math.max(best, endingHere);
  }
  return best;
};
