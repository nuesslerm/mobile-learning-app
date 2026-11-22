var canAttendMeetings = function(intervals) {
  const sorted = [...intervals].sort((a, b) => a[0] - b[0]);
  for (let i = 1; i < sorted.length; i += 1) {
    if (sorted[i][0] < sorted[i - 1][1]) return false;
  }
  return true;
};
