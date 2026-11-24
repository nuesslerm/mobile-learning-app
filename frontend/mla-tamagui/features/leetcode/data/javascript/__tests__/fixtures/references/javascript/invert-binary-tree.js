var invertTree = function(root) {
  if (root) {
    const left = invertTree(root.right);
    const right = invertTree(root.left);
    root.left = left;
    root.right = right;
  }
  return root;
};
