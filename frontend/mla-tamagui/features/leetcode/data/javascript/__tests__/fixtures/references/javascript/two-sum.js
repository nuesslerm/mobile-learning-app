class Solution {
  twoSum(nums, target) {
    const seen = new Map();
    for (let i = 0; i < nums.length; i += 1) {
      if (seen.has(target - nums[i])) return [seen.get(target - nums[i]), i];
      seen.set(nums[i], i);
    }
    return [];
  }
}
