class Solution:
    def canAttendMeetings(self, intervals: List[List[int]]) -> bool:
        ordered = sorted(intervals)
        return all(a[1] <= b[0] for a, b in zip(ordered, ordered[1:]))
