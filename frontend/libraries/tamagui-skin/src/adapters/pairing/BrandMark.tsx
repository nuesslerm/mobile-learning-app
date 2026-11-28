import { BRAND_MARK } from '@lib/skin';
import { useTheme } from '@lib/tamagui-design-system/primitives';
import Svg, { G, Path } from 'react-native-svg';

export function BrandMark({ size = 24 }: { size?: number }) {
  const ink = useTheme().color?.get() as string;
  return (
    <Svg width={size * BRAND_MARK.aspectRatio} height={size} viewBox={BRAND_MARK.viewBox}>
      <G transform={BRAND_MARK.transform}>
        <Path fill={ink} d={BRAND_MARK.path} />
      </G>
    </Svg>
  );
}
