var WordDictionary = function() {
  this.root = { children: new Map(), isWord: false };
};

WordDictionary.prototype.addWord = function(word) {
  let node = this.root;
  for (const ch of word) {
    if (!node.children.has(ch)) node.children.set(ch, { children: new Map(), isWord: false });
    node = node.children.get(ch);
  }
  node.isWord = true;
};

WordDictionary.prototype.search = function(word) {
  const matches = (node, at) => {
    if (at === word.length) return node.isWord;
    if (word[at] === '.') {
      for (const child of node.children.values()) {
        if (matches(child, at + 1)) return true;
      }
      return false;
    }
    const child = node.children.get(word[at]);
    return child !== undefined && matches(child, at + 1);
  };
  return matches(this.root, 0);
};
