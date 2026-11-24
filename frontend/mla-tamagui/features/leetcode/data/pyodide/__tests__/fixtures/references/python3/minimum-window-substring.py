from collections import Counter


class Solution:
    def minWindow(self, s: str, t: str) -> str:
        need, missing = Counter(t), len(t)
        best, lo = '', 0
        for hi, ch in enumerate(s):
            if need[ch] > 0:
                missing -= 1
            need[ch] -= 1
            while missing == 0:
                if not best or hi - lo + 1 < len(best):
                    best = s[lo:hi + 1]
                need[s[lo]] += 1
                if need[s[lo]] > 0:
                    missing += 1
                lo += 1
        return best
