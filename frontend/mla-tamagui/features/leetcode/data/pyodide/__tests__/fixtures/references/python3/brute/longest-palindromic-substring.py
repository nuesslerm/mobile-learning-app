class Solution:
    def longestPalindrome(self, s: str) -> str:
        best, longest = set(), 0
        for i in range(len(s)):
            for j in range(i + 1, len(s) + 1):
                part = s[i:j]
                if part != part[::-1]:
                    continue
                if len(part) > longest:
                    best, longest = {part}, len(part)
                elif len(part) == longest:
                    best.add(part)
        if len(best) != 1:
            raise ValueError('the longest palindrome is not unique: %r' % sorted(best))
        return best.pop()
