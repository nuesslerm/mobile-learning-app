class Solution:
    def productExceptSelf(self, nums: List[int]) -> List[int]:
        n=len(nums); out=[1]*n; p=1
        for i in range(n): out[i]=p; p*=nums[i]
        p=1
        for i in range(n-1,-1,-1): out[i]*=p; p*=nums[i]
        return out
