import { ScrollViewStyleReset, useServerDocumentContext } from 'expo-router/html';
import type { PropsWithChildren } from 'react';

const VIEWPORT_CONTENT =
  'width=device-width, initial-scale=1, minimum-scale=1, maximum-scale=1, user-scalable=no, viewport-fit=cover';

// Expo's own `#expo-reset` sets height and overflow but NOT margin, so the
// UA's default `body { margin: 8px }` survives: the app lays out in a box 16px
// narrower than the viewport, and every full-bleed overlay stops short of the
// right edge. App #1 never showed it because Tailwind's preflight resets the
// margin for free -- which is exactly the freebie a Tamagui app does not get.
const RESET = 'body{margin:0}';

// SDK 58 no longer splices the entry scripts, stylesheets and fonts into the HTML text: it hands them to the root as
// head and body nodes, so a root that renders neither ships a page whose JavaScript never loads.
export default function Root({ children }: PropsWithChildren) {
  const { htmlAttributes, bodyAttributes, headNodes, bodyNodes } = useServerDocumentContext();
  return (
    <html lang="en" {...htmlAttributes}>
      <head>
        <meta charSet="utf-8" />
        <meta httpEquiv="X-UA-Compatible" content="IE=edge" />
        <meta name="viewport" content={VIEWPORT_CONTENT} />
        <ScrollViewStyleReset />
        <style dangerouslySetInnerHTML={{ __html: RESET }} />
        {headNodes}
      </head>
      <body {...bodyAttributes}>
        {children}
        {bodyNodes}
      </body>
    </html>
  );
}
