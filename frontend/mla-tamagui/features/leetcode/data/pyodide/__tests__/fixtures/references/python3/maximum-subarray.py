class Solution:
    def maxSubArray(self, nums: List[int]) -> int:
        best=cur=nums[0]
        for n in nums[1:]:
            cur=max(n,cur+n); best=max(best,cur)
        return best
