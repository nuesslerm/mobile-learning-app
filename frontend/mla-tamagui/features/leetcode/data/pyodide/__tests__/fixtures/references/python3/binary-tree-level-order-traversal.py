from collections import deque
class Solution:
    def levelOrder(self, root):
        out, q = [], deque([root] if root else [])
        while q:
            out.append([n.val for n in q])
            for _ in range(len(q)):
                n = q.popleft()
                for c in (n.left, n.right):
                    if c:
                        q.append(c)
        return out
