var lengthOfLIS = function(nums) {
  const tails = [];
  for (const n of nums) {
    let lo = 0;
    let hi = tails.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (tails[mid] < n) lo = mid + 1;
      else hi = mid;
    }
    tails[lo] = n;
  }
  return tails.length;
};
