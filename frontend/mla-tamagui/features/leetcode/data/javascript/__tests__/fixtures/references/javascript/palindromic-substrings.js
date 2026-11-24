var countSubstrings = function(s) {
  let count = 0;
  const expand = (left, right) => {
    while (left >= 0 && right < s.length && s[left] === s[right]) {
      count += 1;
      left -= 1;
      right += 1;
    }
  };
  for (let center = 0; center < s.length; center += 1) {
    expand(center, center);
    expand(center, center + 1);
  }
  return count;
};
