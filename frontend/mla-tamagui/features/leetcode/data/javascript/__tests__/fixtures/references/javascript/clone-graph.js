var cloneGraph = function(node) {
  if (node === null) return null;
  const copies = new Map([[node, new _Node(node.val)]]);
  const queue = [node];
  for (let at = 0; at < queue.length; at += 1) {
    const current = queue[at];
    for (const neighbour of current.neighbors) {
      if (!copies.has(neighbour)) {
        copies.set(neighbour, new _Node(neighbour.val));
        queue.push(neighbour);
      }
      copies.get(current).neighbors.push(copies.get(neighbour));
    }
  }
  return copies.get(node);
};
