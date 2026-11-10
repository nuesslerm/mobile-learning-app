import {
  usePickerItemHeight,
  useScrollContentOffset,
} from '@quidone/react-native-wheel-picker';
import type {
  PickerItem,
  RenderItemContainer,
} from '@quidone/react-native-wheel-picker';
import { useMemo, type Key } from 'react';
import { Animated, Pressable } from 'react-native';

/**
 * The wheel's own fade is too aggressive to read on a dark theme.
 *
 * `createFaces` hardcodes the neighbour row at **opacity 0.2** (its `getOpacity`
 * map, keyed by distance from centre). White text at 0.2 alpha over the sheet's
 * surface measures ~1.5:1 -- below every legibility threshold there is. With two
 * languages and `visibleItemCount={3}` the neighbour IS the other option, so the
 * choice the dial exists to offer is the one you cannot see.
 *
 * The opacity is applied by the item container, which multiplies with anything
 * the row itself sets, so no colour or style on our side can recover it. What
 * the library does expose is `renderItemContainer` plus the two context hooks
 * this needs -- so this is the default container with one number changed, not a
 * fork: same interpolation, same transforms, same tap-to-scroll.
 */
const MIN_OPACITY = 0.55;

type Props<ItemT extends PickerItem<unknown>> = Parameters<
  RenderItemContainer<ItemT>
>[0];

/**
 * Passed as `renderItemContainer`. Generic in the item type so the picker's own
 * inference still reaches `onValueChanged` -- pinning it to a concrete shape
 * widens `value` back to `string` and the language union is lost.
 */
export function wheelItemContainer<ItemT extends PickerItem<unknown>>(
  props: Props<ItemT>
) {
  // the picker hands its row key inside props; React takes a key only as its own attribute
  const { key, ...rest } = props as Props<ItemT> & { key?: Key };
  return <WheelItemContainer key={key} {...rest} />;
}

function WheelItemContainer<ItemT extends PickerItem<unknown>>({
  listRef,
  item,
  index,
  faces,
  renderItem,
  itemTextStyle,
  enableScrollByTapOnItem,
  readOnly,
}: Props<ItemT>) {
  const offset = useScrollContentOffset();
  const height = usePickerItemHeight();

  const { opacity, rotateX, translateY } = useMemo(() => {
    const inputRange = faces.map((f) => height * (index + f.index));
    return {
      opacity: offset.interpolate({
        inputRange,
        // The only departure from the default container: a floor, so a row that
        // is off-centre is dimmed rather than erased.
        outputRange: faces.map((f) => Math.max(f.opacity, MIN_OPACITY)),
        extrapolate: 'clamp',
      }),
      rotateX: offset.interpolate({
        inputRange,
        outputRange: faces.map((f) => `${f.deg}deg`),
        extrapolate: 'extend',
      }),
      translateY: offset.interpolate({
        inputRange,
        outputRange: faces.map((f) => f.offsetY),
        extrapolate: 'extend',
      }),
    };
  }, [faces, height, index, offset]);

  const view = (
    <Animated.View
      style={{
        height,
        opacity,
        transform: [
          { translateY },
          { rotateX },
          // Android drops the rotation without an explicit perspective.
          { perspective: 1000 },
        ],
      }}
    >
      {renderItem({ item, index, itemTextStyle })}
    </Animated.View>
  );

  if (!enableScrollByTapOnItem || readOnly) return view;

  return (
    <Pressable
      onPress={() => listRef.current?.scrollToIndex({ index, animated: true })}
      testID={`WheelItemContainer-${index}`}
    >
      {view}
    </Pressable>
  );
}
