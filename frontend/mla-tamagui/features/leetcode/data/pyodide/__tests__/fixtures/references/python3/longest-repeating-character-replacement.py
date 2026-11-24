from collections import defaultdict


class Solution:
    def characterReplacement(self, s: str, k: int) -> int:
        count, lo, top, best = defaultdict(int), 0, 0, 0
        for hi, ch in enumerate(s):
            count[ch] += 1
            top = max(top, count[ch])
            while hi - lo + 1 - top > k:
                count[s[lo]] -= 1
                lo += 1
            best = max(best, hi - lo + 1)
        return best
