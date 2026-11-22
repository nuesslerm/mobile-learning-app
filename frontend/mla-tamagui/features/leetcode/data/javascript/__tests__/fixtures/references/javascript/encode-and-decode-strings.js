var encode = function(strs) {
  return strs.map((s) => `${s.length}#${s}`).join('');
};

var decode = function(s) {
  const strs = [];
  let at = 0;
  while (at < s.length) {
    const hash = s.indexOf('#', at);
    const length = Number(s.slice(at, hash));
    strs.push(s.slice(hash + 1, hash + 1 + length));
    at = hash + 1 + length;
  }
  return strs;
};
