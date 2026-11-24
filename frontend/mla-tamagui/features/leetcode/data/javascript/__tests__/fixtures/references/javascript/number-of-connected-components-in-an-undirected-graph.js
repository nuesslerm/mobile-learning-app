var countComponents = function(n, edges) {
  const parent = Array.from({ length: n }, (_, i) => i);
  const find = (node) => {
    while (parent[node] !== node) {
      parent[node] = parent[parent[node]];
      node = parent[node];
    }
    return node;
  };
  let components = n;
  for (const [a, b] of edges) {
    const rootA = find(a);
    const rootB = find(b);
    if (rootA !== rootB) {
      parent[rootA] = rootB;
      components -= 1;
    }
  }
  return components;
};
