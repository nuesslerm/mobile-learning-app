var validTree = function(n, edges) {
  if (edges.length !== n - 1) return false;
  const parent = Array.from({ length: n }, (_, i) => i);
  const find = (node) => {
    while (parent[node] !== node) {
      parent[node] = parent[parent[node]];
      node = parent[node];
    }
    return node;
  };
  for (const [a, b] of edges) {
    const rootA = find(a);
    const rootB = find(b);
    if (rootA === rootB) return false;
    parent[rootA] = rootB;
  }
  return true;
};
