var longestCommonSubsequence = function(text1, text2) {
  const lengths = Array.from({ length: text1.length + 1 }, () => new Array(text2.length + 1).fill(0));
  for (let i = 1; i <= text1.length; i += 1) {
    for (let j = 1; j <= text2.length; j += 1) {
      lengths[i][j] = text1[i - 1] === text2[j - 1]
        ? lengths[i - 1][j - 1] + 1
        : Math.max(lengths[i - 1][j], lengths[i][j - 1]);
    }
  }
  return lengths[text1.length][text2.length];
};
