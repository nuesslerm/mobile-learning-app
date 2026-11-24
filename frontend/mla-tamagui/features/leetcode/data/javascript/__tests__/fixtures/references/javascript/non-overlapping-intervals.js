var eraseOverlapIntervals = function(intervals) {
  const sorted = [...intervals].sort((a, b) => a[1] - b[1]);
  let removed = 0;
  let lastEnd = -Infinity;
  for (const [start, end] of sorted) {
    if (start >= lastEnd) lastEnd = end;
    else removed += 1;
  }
  return removed;
};
