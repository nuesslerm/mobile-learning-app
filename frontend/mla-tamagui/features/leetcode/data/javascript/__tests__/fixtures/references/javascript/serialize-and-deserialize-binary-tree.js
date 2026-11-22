var serialize = function(root) {
  const out = [];
  const queue = [root];
  for (let at = 0; at < queue.length; at += 1) {
    const node = queue[at];
    if (node === null) {
      out.push('#');
    } else {
      out.push(String(node.val));
      queue.push(node.left, node.right);
    }
  }
  return out.join(',');
};

var deserialize = function(data) {
  const values = data.split(',');
  if (values[0] === '#') return null;
  const root = new TreeNode(Number(values[0]));
  const queue = [root];
  let i = 1;
  for (let at = 0; at < queue.length; at += 1) {
    const node = queue[at];
    if (values[i] !== '#') {
      node.left = new TreeNode(Number(values[i]));
      queue.push(node.left);
    }
    if (values[i + 1] !== '#') {
      node.right = new TreeNode(Number(values[i + 1]));
      queue.push(node.right);
    }
    i += 2;
  }
  return root;
};
