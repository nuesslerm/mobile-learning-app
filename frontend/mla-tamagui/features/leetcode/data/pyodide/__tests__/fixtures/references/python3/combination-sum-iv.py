class Solution:
    def combinationSum4(self, nums: List[int], target: int) -> int:
        ways = [1] + [0] * target
        for total in range(1, target + 1):
            ways[total] = sum(ways[total - n] for n in nums if n <= total)
        return ways[target]
