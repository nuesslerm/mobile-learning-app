class Solution:
    def longestConsecutive(self, nums: List[int]) -> int:
        seen = set(nums)
        best = 0
        for n in seen:
            if n - 1 not in seen:
                end = n
                while end + 1 in seen:
                    end += 1
                best = max(best, end - n + 1)
        return best
