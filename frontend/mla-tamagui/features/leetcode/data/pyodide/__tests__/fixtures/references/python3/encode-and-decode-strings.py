class Solution:
    def encode(self, strs: List[str]) -> str:
        return ''.join(str(len(s)) + '#' + s for s in strs)

    def decode(self, s: str) -> List[str]:
        out, i = [], 0
        while i < len(s):
            j = s.index('#', i)
            size = int(s[i:j])
            out.append(s[j + 1:j + 1 + size])
            i = j + 1 + size
        return out
