var pacificAtlantic = function(heights) {
  const rows = heights.length;
  const cols = heights[0].length;
  const reachFrom = (starts) => {
    const seen = new Set(starts.map(([r, c]) => r * cols + c));
    const stack = [...starts];
    while (stack.length > 0) {
      const [r, c] = stack.pop();
      for (const [dr, dc] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
        const nr = r + dr;
        const nc = c + dc;
        if (nr < 0 || nr >= rows || nc < 0 || nc >= cols) continue;
        if (seen.has(nr * cols + nc) || heights[nr][nc] < heights[r][c]) continue;
        seen.add(nr * cols + nc);
        stack.push([nr, nc]);
      }
    }
    return seen;
  };
  const pacificEdge = [];
  const atlanticEdge = [];
  for (let r = 0; r < rows; r += 1) {
    pacificEdge.push([r, 0]);
    atlanticEdge.push([r, cols - 1]);
  }
  for (let c = 0; c < cols; c += 1) {
    pacificEdge.push([0, c]);
    atlanticEdge.push([rows - 1, c]);
  }
  const pacific = reachFrom(pacificEdge);
  const atlantic = reachFrom(atlanticEdge);
  const both = [];
  for (let r = 0; r < rows; r += 1) {
    for (let c = 0; c < cols; c += 1) {
      if (pacific.has(r * cols + c) && atlantic.has(r * cols + c)) both.push([r, c]);
    }
  }
  return both;
};
