class Solution:
    def rob(self, nums: List[int]) -> int:
        def line(houses):
            take = skip = 0
            for h in houses:
                take, skip = skip + h, max(take, skip)
            return max(take, skip)

        return nums[0] if len(nums) == 1 else max(line(nums[1:]), line(nums[:-1]))
