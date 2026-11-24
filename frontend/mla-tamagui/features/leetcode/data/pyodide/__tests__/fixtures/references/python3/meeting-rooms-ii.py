import heapq


class Solution:
    def minMeetingRooms(self, intervals: List[List[int]]) -> int:
        ends = []
        for start, end in sorted(intervals):
            if ends and ends[0] <= start:
                heapq.heapreplace(ends, end)
            else:
                heapq.heappush(ends, end)
        return len(ends)
