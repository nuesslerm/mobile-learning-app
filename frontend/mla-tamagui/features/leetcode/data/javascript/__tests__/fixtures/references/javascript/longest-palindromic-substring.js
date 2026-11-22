var longestPalindrome = function(s) {
  let start = 0;
  let length = 0;
  const expand = (left, right) => {
    while (left >= 0 && right < s.length && s[left] === s[right]) {
      left -= 1;
      right += 1;
    }
    if (right - left - 1 > length) {
      start = left + 1;
      length = right - left - 1;
    }
  };
  for (let center = 0; center < s.length; center += 1) {
    expand(center, center);
    expand(center, center + 1);
  }
  return s.slice(start, start + length);
};
