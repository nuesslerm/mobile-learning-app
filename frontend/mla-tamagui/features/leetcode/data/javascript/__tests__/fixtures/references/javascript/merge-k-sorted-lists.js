function mergeSorted(a, b) {
  const dummy = new ListNode(0);
  let tail = dummy;
  while (a && b) {
    if (a.val <= b.val) {
      tail.next = a;
      a = a.next;
    } else {
      tail.next = b;
      b = b.next;
    }
    tail = tail.next;
  }
  tail.next = a || b;
  return dummy.next;
}

var mergeKLists = function(lists) {
  let round = lists;
  while (round.length > 1) {
    const merged = [];
    for (let i = 0; i < round.length; i += 2) {
      merged.push(mergeSorted(round[i], round[i + 1] || null));
    }
    round = merged;
  }
  return round.length === 0 ? null : round[0];
};
