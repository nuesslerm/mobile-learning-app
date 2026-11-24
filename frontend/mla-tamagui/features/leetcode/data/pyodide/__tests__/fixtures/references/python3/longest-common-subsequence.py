class Solution:
    def longestCommonSubsequence(self, text1: str, text2: str) -> int:
        prev = [0] * (len(text2) + 1)
        for a in text1:
            row = [0]
            for j, b in enumerate(text2):
                row.append(prev[j] + 1 if a == b else max(prev[j + 1], row[j]))
            prev = row
        return prev[-1]
