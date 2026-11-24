var kthSmallest = function(root, k) {
  const stack = [];
  let node = root;
  while (node || stack.length > 0) {
    while (node) {
      stack.push(node);
      node = node.left;
    }
    node = stack.pop();
    k -= 1;
    if (k === 0) return node.val;
    node = node.right;
  }
  return -1;
};
