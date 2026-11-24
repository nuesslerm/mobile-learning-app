import heapq
class Solution:
    def mergeKLists(self, lists):
        heap = [(n.val, i, n) for i, n in enumerate(lists) if n]
        heapq.heapify(heap)
        dummy = tail = ListNode()
        while heap:
            _, i, node = heapq.heappop(heap)
            tail.next = node
            tail = node
            if node.next:
                heapq.heappush(heap, (node.next.val, i, node.next))
        return dummy.next
