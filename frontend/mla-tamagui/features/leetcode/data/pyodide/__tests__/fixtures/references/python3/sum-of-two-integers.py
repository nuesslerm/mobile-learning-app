class Solution:
    def getSum(self, a: int, b: int) -> int:
        mask, top = 0xFFFFFFFF, 0x7FFFFFFF
        while b:
            a, b = (a ^ b) & mask, ((a & b) << 1) & mask
        return a if a <= top else ~(a ^ mask)
