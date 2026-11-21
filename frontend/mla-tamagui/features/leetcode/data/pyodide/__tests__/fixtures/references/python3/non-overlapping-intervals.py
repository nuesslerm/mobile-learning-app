class Solution:
    def eraseOverlapIntervals(self, intervals: List[List[int]]) -> int:
        removed, end = 0, float('-inf')
        for start, stop in sorted(intervals, key=lambda x: x[1]):
            if start >= end:
                end = stop
            else:
                removed += 1
        return removed
