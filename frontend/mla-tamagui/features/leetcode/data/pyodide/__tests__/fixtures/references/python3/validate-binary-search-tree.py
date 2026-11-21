class Solution:
    def isValidBST(self, root):
        def ok(n, lo, hi):
            return not n or (lo < n.val < hi and ok(n.left, lo, n.val) and ok(n.right, n.val, hi))
        return ok(root, float('-inf'), float('inf'))
