function sameTree(a, b) {
  if (!a || !b) return a === b;
  return a.val === b.val && sameTree(a.left, b.left) && sameTree(a.right, b.right);
}

var isSubtree = function(root, subRoot) {
  if (!root) return !subRoot;
  return sameTree(root, subRoot) || isSubtree(root.left, subRoot) || isSubtree(root.right, subRoot);
};
