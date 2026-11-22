var setZeroes = function(matrix) {
  const zeroRows = new Set();
  const zeroCols = new Set();
  for (let r = 0; r < matrix.length; r += 1) {
    for (let c = 0; c < matrix[r].length; c += 1) {
      if (matrix[r][c] === 0) {
        zeroRows.add(r);
        zeroCols.add(c);
      }
    }
  }
  for (let r = 0; r < matrix.length; r += 1) {
    for (let c = 0; c < matrix[r].length; c += 1) {
      if (zeroRows.has(r) || zeroCols.has(c)) matrix[r][c] = 0;
    }
  }
};
