var alienOrder = function(words) {
  const successors = new Map();
  for (const word of words) {
    for (const ch of word) {
      if (!successors.has(ch)) successors.set(ch, new Set());
    }
  }
  for (let i = 0; i + 1 < words.length; i += 1) {
    const a = words[i];
    const b = words[i + 1];
    if (a.length > b.length && a.startsWith(b)) return '';
    for (let j = 0; j < Math.min(a.length, b.length); j += 1) {
      if (a[j] !== b[j]) {
        successors.get(a[j]).add(b[j]);
        break;
      }
    }
  }
  const blockers = new Map([...successors.keys()].map((ch) => [ch, 0]));
  for (const after of successors.values()) {
    for (const ch of after) blockers.set(ch, blockers.get(ch) + 1);
  }
  const ready = [...blockers.keys()].filter((ch) => blockers.get(ch) === 0);
  let order = '';
  while (ready.length > 0) {
    const ch = ready.shift();
    order += ch;
    for (const next of successors.get(ch)) {
      blockers.set(next, blockers.get(next) - 1);
      if (blockers.get(next) === 0) ready.push(next);
    }
  }
  return order.length === successors.size ? order : '';
};
