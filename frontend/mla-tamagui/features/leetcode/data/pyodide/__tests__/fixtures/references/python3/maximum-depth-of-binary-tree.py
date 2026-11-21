class Solution:
    def maxDepth(self, root):
        return 0 if not root else 1 + max(self.maxDepth(root.left), self.maxDepth(root.right))
