import { expect, it } from 'vitest';

import { getStrokeGuide, guidedCharacterIds } from '../../domain/strokeGuide';
import { assessStroke } from '../../domain/strokeValidation';
import { traceGuideValue } from '../traceGuideValue';

it('Given the iPhone tracing pad, when XCTest reads each stroke, then it fits the accessibility value and passes scoring', () => {
  const canvas = { width: 402, height: 300 };
  for (const id of guidedCharacterIds()) {
    for (const stroke of getStrokeGuide(id)!.strokes) {
      const line = stroke.points.map(({ x, y }) => ({ x: x * canvas.width, y: y * canvas.height }));
      const value = traceGuideValue(line);
      expect(value.length).toBeLessThanOrEqual(512);
      const { guide } = JSON.parse(value) as { guide: [Array<[number, number]>] };
      const drawn = guide[0].map(([x, y]) => ({ x, y }));
      expect(assessStroke(drawn, stroke, canvas).ok).toBe(true);
    }
  }
});
