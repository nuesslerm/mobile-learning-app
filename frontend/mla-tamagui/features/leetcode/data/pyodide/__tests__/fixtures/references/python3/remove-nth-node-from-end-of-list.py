class Solution:
    def removeNthFromEnd(self, head, n):
        dummy = ListNode(0, head)
        fast = slow = dummy
        for _ in range(n):
            fast = fast.next
        while fast.next:
            fast, slow = fast.next, slow.next
        slow.next = slow.next.next
        return dummy.next
