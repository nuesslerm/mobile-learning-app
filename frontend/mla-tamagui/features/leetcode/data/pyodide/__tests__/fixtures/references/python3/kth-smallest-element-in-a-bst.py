class Solution:
    def kthSmallest(self, root, k):
        out = []
        def walk(n):
            if n and len(out) < k:
                walk(n.left)
                out.append(n.val)
                walk(n.right)
        walk(root)
        return out[k - 1]
