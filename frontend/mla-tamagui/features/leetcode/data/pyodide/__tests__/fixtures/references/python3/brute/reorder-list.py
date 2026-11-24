class Solution:
    def reorderList(self, head: Optional[ListNode]) -> None:
        nodes = []
        node = head
        while node is not None:
            nodes.append(node)
            node = node.next
        if not nodes:
            return
        lo, hi = 0, len(nodes) - 1
        while lo < hi:
            nodes[lo].next = nodes[hi]
            lo += 1
            if lo == hi:
                break
            nodes[hi].next = nodes[lo]
            hi -= 1
        nodes[lo].next = None
