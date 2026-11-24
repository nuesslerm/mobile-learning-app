class Solution:
    def longestPalindrome(self, s: str) -> str:
        start, end = 0, 0
        for center in range(len(s)):
            for lo, hi in ((center, center), (center, center + 1)):
                while lo >= 0 and hi < len(s) and s[lo] == s[hi]:
                    lo -= 1
                    hi += 1
                if hi - lo - 1 > end - start:
                    start, end = lo + 1, hi
        return s[start:end]
