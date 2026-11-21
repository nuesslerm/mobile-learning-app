class Solution:
    def buildTree(self, preorder, inorder):
        idx = {v: i for i, v in enumerate(inorder)}
        it = iter(preorder)
        def build(lo, hi):
            if lo > hi:
                return None
            node = TreeNode(next(it))
            m = idx[node.val]
            node.left = build(lo, m - 1)
            node.right = build(m + 1, hi)
            return node
        return build(0, len(inorder) - 1)
