var minMeetingRooms = function(intervals) {
  const starts = intervals.map(([start]) => start).sort((a, b) => a - b);
  const ends = intervals.map(([, end]) => end).sort((a, b) => a - b);
  let rooms = 0;
  let nextToEnd = 0;
  for (const start of starts) {
    if (start >= ends[nextToEnd]) nextToEnd += 1;
    else rooms += 1;
  }
  return rooms;
};
