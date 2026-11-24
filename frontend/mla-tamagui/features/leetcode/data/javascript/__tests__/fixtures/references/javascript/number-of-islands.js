var numIslands = function(grid) {
  const rows = grid.length;
  const cols = rows === 0 ? 0 : grid[0].length;
  const sink = (r, c) => {
    if (r < 0 || r >= rows || c < 0 || c >= cols || grid[r][c] !== '1') return;
    grid[r][c] = '0';
    sink(r + 1, c);
    sink(r - 1, c);
    sink(r, c + 1);
    sink(r, c - 1);
  };
  let islands = 0;
  for (let r = 0; r < rows; r += 1) {
    for (let c = 0; c < cols; c += 1) {
      if (grid[r][c] === '1') {
        islands += 1;
        sink(r, c);
      }
    }
  }
  return islands;
};
