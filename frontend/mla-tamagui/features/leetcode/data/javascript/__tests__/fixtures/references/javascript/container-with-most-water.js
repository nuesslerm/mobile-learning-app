var maxArea = function(height) {
  let lo = 0;
  let hi = height.length - 1;
  let best = 0;
  while (lo < hi) {
    best = Math.max(best, (hi - lo) * Math.min(height[lo], height[hi]));
    if (height[lo] < height[hi]) lo += 1;
    else hi -= 1;
  }
  return best;
};
