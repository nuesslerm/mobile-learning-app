class Heap {
  constructor(before) {
    this.before = before;
    this.items = [];
  }

  get size() {
    return this.items.length;
  }

  peek() {
    return this.items[0];
  }

  push(value) {
    const items = this.items;
    items.push(value);
    let at = items.length - 1;
    while (at > 0) {
      const parent = (at - 1) >> 1;
      if (!this.before(items[at], items[parent])) break;
      [items[at], items[parent]] = [items[parent], items[at]];
      at = parent;
    }
  }

  pop() {
    const items = this.items;
    const top = items[0];
    const last = items.pop();
    if (items.length > 0) {
      items[0] = last;
      let at = 0;
      for (;;) {
        const left = 2 * at + 1;
        const right = left + 1;
        let first = at;
        if (left < items.length && this.before(items[left], items[first])) first = left;
        if (right < items.length && this.before(items[right], items[first])) first = right;
        if (first === at) break;
        [items[at], items[first]] = [items[first], items[at]];
        at = first;
      }
    }
    return top;
  }
}

var MedianFinder = function() {
  this.low = new Heap((a, b) => a > b);
  this.high = new Heap((a, b) => a < b);
};

MedianFinder.prototype.addNum = function(num) {
  this.low.push(num);
  this.high.push(this.low.pop());
  if (this.high.size > this.low.size) this.low.push(this.high.pop());
};

MedianFinder.prototype.findMedian = function() {
  if (this.low.size > this.high.size) return this.low.peek();
  return (this.low.peek() + this.high.peek()) / 2;
};
