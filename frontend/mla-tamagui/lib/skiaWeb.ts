import type * as SkiaWeb from '@shopify/react-native-skia/lib/module/web';

// CanvasKit's web loader pulls in `fs` and cannot be bundled for iOS or Android; the Skia canvas only calls these
// when Platform.OS is 'web', where skiaWeb.web.ts is the real module.
const webOnly = (): never => {
  throw new Error('Skia web loader is web-only');
};

export const LoadSkiaWeb: typeof SkiaWeb.LoadSkiaWeb = webOnly;
export const WithSkiaWeb: typeof SkiaWeb.WithSkiaWeb = webOnly;
