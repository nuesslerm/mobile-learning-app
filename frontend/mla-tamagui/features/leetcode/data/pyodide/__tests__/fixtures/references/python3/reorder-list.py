class Solution:
    def reorderList(self, head: Optional[ListNode]) -> None:
        if head is None or head.next is None:
            return
        slow, fast = head, head.next
        while fast and fast.next:
            slow = slow.next
            fast = fast.next.next
        second, slow.next = slow.next, None
        prev = None
        while second:
            second.next, prev, second = prev, second, second.next
        first, second = head, prev
        while second:
            first.next, second.next, first, second = second, first.next, first.next, second.next
