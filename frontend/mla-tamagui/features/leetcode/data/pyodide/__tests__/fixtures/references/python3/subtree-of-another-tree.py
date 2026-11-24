class Solution:
    def isSubtree(self, root, subRoot):
        def same(a, b):
            if not a and not b:
                return True
            return bool(a and b and a.val == b.val and same(a.left, b.left) and same(a.right, b.right))
        if not subRoot:
            return True
        if not root:
            return False
        return same(root, subRoot) or self.isSubtree(root.left, subRoot) or self.isSubtree(root.right, subRoot)
