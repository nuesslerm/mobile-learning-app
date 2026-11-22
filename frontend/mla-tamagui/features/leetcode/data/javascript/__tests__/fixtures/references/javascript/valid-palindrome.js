var isPalindrome = function(s) {
  const letters = s.toLowerCase().replace(/[^a-z0-9]/g, '');
  return letters === [...letters].reverse().join('');
};
