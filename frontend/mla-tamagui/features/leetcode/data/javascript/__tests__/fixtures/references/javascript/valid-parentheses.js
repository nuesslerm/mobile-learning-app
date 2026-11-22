var isValid = function(s) {
  const open = { ')': '(', ']': '[', '}': '{' };
  const stack = [];
  for (const c of s) {
    if (c in open) {
      if (stack.pop() !== open[c]) return false;
    } else {
      stack.push(c);
    }
  }
  return stack.length === 0;
};
