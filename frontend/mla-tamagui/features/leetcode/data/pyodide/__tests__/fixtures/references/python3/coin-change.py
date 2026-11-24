class Solution:
    def coinChange(self, coins: List[int], amount: int) -> int:
        dp=[0]+[amount+1]*amount
        for a in range(1,amount+1):
            for c in coins:
                if c<=a: dp[a]=min(dp[a],dp[a-c]+1)
        return dp[amount] if dp[amount]<=amount else -1
