import { BRAND_MARK } from '@lib/skin';
import Svg, { G, Path, Defs } from 'react-native-svg';
import { homePalettes, useOrcaScheme } from './appearance';

export function BrandMark({ size = 24 }: { size?: number }) {
  const color = homePalettes[useOrcaScheme()].textPrimary;
  return (
    <Svg width={size * BRAND_MARK.aspectRatio} height={size} viewBox={BRAND_MARK.viewBox}>
      <Defs />
      <G transform={BRAND_MARK.transform}>
        <Path fill={color} d={BRAND_MARK.path} />
      </G>
    </Svg>
  );
}
