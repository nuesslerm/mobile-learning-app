var levelOrder = function(root) {
  const levels = [];
  let row = root ? [root] : [];
  while (row.length > 0) {
    levels.push(row.map((node) => node.val));
    row = row.flatMap((node) => [node.left, node.right].filter(Boolean));
  }
  return levels;
};
