class Solution:
    def numDecodings(self, s: str) -> int:
        one, two = 1, 0
        for i, ch in enumerate(s):
            now = 0
            if ch != '0':
                now += one
            if i and 10 <= int(s[i - 1:i + 1]) <= 26:
                now += two
            one, two = now, one
        return one
