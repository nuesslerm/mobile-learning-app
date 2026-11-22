var threeSum = function(nums) {
  const sorted = [...nums].sort((a, b) => a - b);
  const triples = [];
  for (let i = 0; i < sorted.length - 2; i += 1) {
    if (sorted[i] > 0) break;
    if (i > 0 && sorted[i] === sorted[i - 1]) continue;
    let lo = i + 1;
    let hi = sorted.length - 1;
    while (lo < hi) {
      const sum = sorted[i] + sorted[lo] + sorted[hi];
      if (sum < 0) {
        lo += 1;
      } else if (sum > 0) {
        hi -= 1;
      } else {
        triples.push([sorted[i], sorted[lo], sorted[hi]]);
        lo += 1;
        while (lo < hi && sorted[lo] === sorted[lo - 1]) lo += 1;
        hi -= 1;
      }
    }
  }
  return triples;
};
