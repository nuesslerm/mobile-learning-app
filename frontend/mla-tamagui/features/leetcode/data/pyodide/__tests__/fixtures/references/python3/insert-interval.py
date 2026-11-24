class Solution:
    def insert(self, intervals: List[List[int]], newInterval: List[int]) -> List[List[int]]:
        out, lo, hi = [], newInterval[0], newInterval[1]
        i = 0
        while i < len(intervals) and intervals[i][1] < lo:
            out.append(intervals[i])
            i += 1
        while i < len(intervals) and intervals[i][0] <= hi:
            lo, hi = min(lo, intervals[i][0]), max(hi, intervals[i][1])
            i += 1
        out.append([lo, hi])
        out.extend(intervals[i:])
        return out
