import { describe, expect, it } from 'vitest';

import { chipScrollTarget } from '../chipScroll';

describe('chipScrollTarget', () => {
  it('Given a chip inside the viewport, When asked, Then the scroll stays', () => {
    expect(chipScrollTarget({ x: 100, width: 60 }, 300, 50)).toBe(50);
  });

  it('Given a chip left of the viewport, When asked, Then it scrolls left to it', () => {
    expect(chipScrollTarget({ x: 20, width: 60 }, 300, 200)).toBe(8);
  });

  it('Given a chip right of the viewport, When asked, Then it scrolls right until the chip is whole', () => {
    expect(chipScrollTarget({ x: 400, width: 60 }, 300, 0)).toBe(172);
  });

  it('Given a selected case with a trailing remove control, When revealed, Then the right gutter fits without changing the left margin', () => {
    expect(chipScrollTarget({ x: 245, width: 65 }, 302, 20, 28)).toBe(36);
    expect(chipScrollTarget({ x: 20, width: 65 }, 302, 36, 28)).toBe(8);
  });

  it('Given no measured viewport, When asked, Then the scroll stays', () => {
    expect(chipScrollTarget({ x: 400, width: 60 }, 0, 10)).toBe(10);
  });
});
