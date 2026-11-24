from collections import deque


class Codec:
    def serialize(self, root):
        out, queue = [], deque([root])
        while queue:
            node = queue.popleft()
            if node is None:
                out.append('#')
                continue
            out.append(str(node.val))
            queue.append(node.left)
            queue.append(node.right)
        return ','.join(out)

    def deserialize(self, data):
        values = data.split(',')
        if values[0] == '#':
            return None
        root = TreeNode(int(values[0]))
        queue, i = deque([root]), 1
        while queue:
            node = queue.popleft()
            if values[i] != '#':
                node.left = TreeNode(int(values[i]))
                queue.append(node.left)
            if values[i + 1] != '#':
                node.right = TreeNode(int(values[i + 1]))
                queue.append(node.right)
            i += 2
        return root
