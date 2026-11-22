var insert = function(intervals, newInterval) {
  const result = [];
  let [start, end] = newInterval;
  let i = 0;
  while (i < intervals.length && intervals[i][1] < start) {
    result.push(intervals[i]);
    i += 1;
  }
  while (i < intervals.length && intervals[i][0] <= end) {
    start = Math.min(start, intervals[i][0]);
    end = Math.max(end, intervals[i][1]);
    i += 1;
  }
  result.push([start, end]);
  while (i < intervals.length) {
    result.push(intervals[i]);
    i += 1;
  }
  return result;
};
