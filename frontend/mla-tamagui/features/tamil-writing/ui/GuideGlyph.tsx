import { Text, View } from '@lib/tamagui-design-system';
import { useState } from 'react';
import type { LayoutChangeEvent } from 'react-native';

import { guideBaseFontSize, guideFontSize } from '../domain/guideSize';
import { useTamilFont } from './useTamilFont';

/** Wide enough that the measuring copy of a word never wraps. */
const MEASURE_WIDTH = 10_000;

type GuideGlyphProps = {
  glyph: string;
  /** The pad, in pixels. */
  width: number;
  height: number;
  /** How much of the pad's shorter side the glyph takes. */
  scale: number;
  testID: string;
};

/**
 * The faint letter or word under the ink, shrunk to the pad's width when a
 * long word would run past it: a hidden copy at full size gives its width.
 */
export function GuideGlyph({
  glyph,
  width,
  height,
  scale,
  testID,
}: GuideGlyphProps): React.ReactElement {
  const [natural, setNatural] = useState(0);
  const fontReady = useTamilFont();
  const base = guideBaseFontSize({ width, height }, scale);
  const fontSize = guideFontSize({ width, height }, scale, natural);

  return (
    <View
      position="absolute"
      t={0}
      l={0}
      r={0}
      b={0}
      items="center"
      justify="center"
      pointerEvents="none"
      testID={`${testID}Guide`}
    >
      {/* never the platform's font, even for a frame: a trace started then would follow the wrong shape */}
      {fontReady ? (
        <>
          <View
            position="absolute"
            t={0}
            l={0}
            width={MEASURE_WIDTH}
            opacity={0}
            aria-hidden
          >
            <View
              self="flex-start"
              onLayout={(event: LayoutChangeEvent) =>
                setNatural(event.nativeEvent.layout.width)
              }
              testID={`${testID}GuideMeasure`}
            >
              <Text fontFamily="$tamil" fontSize={base} lineHeight={base * 1.1}>
                {glyph}
              </Text>
            </View>
          </View>
          <Text
            fontFamily="$tamil"
            fontSize={fontSize}
            lineHeight={fontSize * 1.1}
            text="center"
            color="$color"
            opacity={0.16}
            testID={`${testID}GuideGlyph`}
          >
            {glyph}
          </Text>
        </>
      ) : null}
    </View>
  );
}
