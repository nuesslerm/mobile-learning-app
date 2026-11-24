class Solution:
    def maxProduct(self, nums: List[int]) -> int:
        best = high = low = nums[0]
        for n in nums[1:]:
            candidates = (n, high * n, low * n)
            high, low = max(candidates), min(candidates)
            best = max(best, high)
        return best
