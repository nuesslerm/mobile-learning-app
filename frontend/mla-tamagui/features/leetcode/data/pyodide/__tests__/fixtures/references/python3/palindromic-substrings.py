class Solution:
    def countSubstrings(self, s: str) -> int:
        count = 0
        for center in range(len(s)):
            for lo, hi in ((center, center), (center, center + 1)):
                while lo >= 0 and hi < len(s) and s[lo] == s[hi]:
                    count += 1
                    lo -= 1
                    hi += 1
        return count
