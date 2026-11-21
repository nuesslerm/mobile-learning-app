class Solution:
    def hasCycle(self, head: Optional[ListNode]) -> bool:
        seen = set()
        while head is not None:
            if id(head) in seen:
                return True
            seen.add(id(head))
            head = head.next
        return False
