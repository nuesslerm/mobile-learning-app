class Solution:
    def lengthOfLongestSubstring(self, s: str) -> int:
        last, start, best = {}, 0, 0
        for i, ch in enumerate(s):
            if ch in last and last[ch] >= start:
                start = last[ch] + 1
            last[ch] = i
            best = max(best, i - start + 1)
        return best
