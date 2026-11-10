import { TracingCanvasSkia } from './TracingCanvasSkia';
import { TracingCanvasSvg } from './TracingCanvasSvg';

export const TracingCanvas =
  process.env.EXPO_PUBLIC_TAMIL_CANVAS === 'skia'
    ? TracingCanvasSkia
    : TracingCanvasSvg;
