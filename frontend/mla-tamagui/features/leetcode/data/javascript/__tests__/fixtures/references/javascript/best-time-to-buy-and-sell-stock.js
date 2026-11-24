var maxProfit = function(prices) {
  let lowest = Infinity;
  let best = 0;
  for (const price of prices) {
    lowest = Math.min(lowest, price);
    best = Math.max(best, price - lowest);
  }
  return best;
};
