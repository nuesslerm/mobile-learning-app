class Solution:
    def maxPathSum(self, root):
        best = [float('-inf')]
        def go(n):
            if not n:
                return 0
            l, r = max(go(n.left), 0), max(go(n.right), 0)
            best[0] = max(best[0], n.val + l + r)
            return n.val + max(l, r)
        go(root)
        return best[0]
