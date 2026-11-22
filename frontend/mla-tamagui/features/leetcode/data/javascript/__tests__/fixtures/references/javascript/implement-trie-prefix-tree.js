var Trie = function() {
  this.root = {};
};

Trie.prototype.insert = function(word) {
  let node = this.root;
  for (const ch of word) {
    node[ch] = node[ch] || {};
    node = node[ch];
  }
  node.end = true;
};

Trie.prototype.walk = function(text) {
  let node = this.root;
  for (const ch of text) {
    node = node[ch];
    if (!node) return null;
  }
  return node;
};

Trie.prototype.search = function(word) {
  const node = this.walk(word);
  return node !== null && node.end === true;
};

Trie.prototype.startsWith = function(prefix) {
  return this.walk(prefix) !== null;
};
