var productExceptSelf = function(nums) {
  const answer = new Array(nums.length);
  let prefix = 1;
  for (let i = 0; i < nums.length; i += 1) {
    answer[i] = prefix;
    prefix *= nums[i];
  }
  let suffix = 1;
  for (let i = nums.length - 1; i >= 0; i -= 1) {
    answer[i] *= suffix;
    suffix *= nums[i];
  }
  return answer;
};
