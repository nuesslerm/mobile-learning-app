type Line = { sku: string; qty: number };

export function totalQuantity(lines: Line[]): number {
  let total = 0;
  for (const line of lines) {
    total += line.qty;
  }
  return total;
}
