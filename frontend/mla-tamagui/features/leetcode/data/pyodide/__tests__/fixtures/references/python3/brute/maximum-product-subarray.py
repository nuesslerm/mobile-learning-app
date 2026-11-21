class Solution:
    def maxProduct(self, nums: List[int]) -> int:
        best = nums[0]
        for i in range(len(nums)):
            product = 1
            for j in range(i, len(nums)):
                product *= nums[j]
                best = max(best, product)
        return best
