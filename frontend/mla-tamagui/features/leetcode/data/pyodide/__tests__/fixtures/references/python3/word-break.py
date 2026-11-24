class Solution:
    def wordBreak(self, s: str, wordDict: List[str]) -> bool:
        words = set(wordDict)
        ok = [True] + [False] * len(s)
        for i in range(1, len(s) + 1):
            ok[i] = any(ok[j] and s[j:i] in words for j in range(i))
        return ok[len(s)]
