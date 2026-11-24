var isValidBST = function(root) {
  const within = (node, lo, hi) => {
    if (!node) return true;
    if (node.val <= lo || node.val >= hi) return false;
    return within(node.left, lo, node.val) && within(node.right, node.val, hi);
  };
  return within(root, -Infinity, Infinity);
};
