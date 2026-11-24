var buildTree = function(preorder, inorder) {
  const inorderIndex = new Map(inorder.map((val, i) => [val, i]));
  let next = 0;
  const build = (lo, hi) => {
    if (lo > hi) return null;
    const node = new TreeNode(preorder[next]);
    next += 1;
    const mid = inorderIndex.get(node.val);
    node.left = build(lo, mid - 1);
    node.right = build(mid + 1, hi);
    return node;
  };
  return build(0, inorder.length - 1);
};
