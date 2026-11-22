var reorderList = function(head) {
  if (!head) return;
  let slow = head;
  let fast = head.next;
  while (fast && fast.next) {
    slow = slow.next;
    fast = fast.next.next;
  }
  let second = slow.next;
  slow.next = null;
  let reversed = null;
  while (second) {
    const next = second.next;
    second.next = reversed;
    reversed = second;
    second = next;
  }
  let first = head;
  while (reversed) {
    const nextFirst = first.next;
    const nextReversed = reversed.next;
    first.next = reversed;
    reversed.next = nextFirst;
    first = nextFirst;
    reversed = nextReversed;
  }
};
