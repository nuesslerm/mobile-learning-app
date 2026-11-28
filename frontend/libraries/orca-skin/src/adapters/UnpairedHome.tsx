import { Fragment } from 'react';
import { ScrollView, Text, View } from 'react-native';
import {
  useSkin,
  type UnpairedHomeProps,
} from '@lib/skin';
import { useHomeStyles } from './homeStyles';

export function UnpairedHome(props: UnpairedHomeProps) {
  const { BrandMark, PairDesktopButton } = useSkin();
  const styles = useHomeStyles();
  return (
    <ScrollView
      contentContainerStyle={[
        styles.container,
        { paddingBottom: props.bottomInset },
        props.isWideLayout && {
          maxWidth: props.contentMaxWidth,
          width: '100%',
          alignSelf: 'center',
        },
      ]}
    >
      <View style={styles.hero}>
        <BrandMark size={44} />
        <Text style={styles.title} accessibilityRole="header">
          {props.title}
        </Text>
        <Text style={styles.body}>{props.description}</Text>
      </View>
      <View style={styles.steps}>
        {props.steps.map((step, index) => (
          <Fragment key={step.id}>
            {index > 0 ? <View style={styles.separator} /> : null}
            <View style={styles.step}>
              <View style={styles.stepNumber}>
                <Text style={styles.stepNumberText}>{index + 1}</Text>
              </View>
              <View style={styles.stepText}>
                <Text style={styles.stepTitle}>{step.title}</Text>
                <Text style={styles.stepDescription}>{step.description}</Text>
              </View>
            </View>
          </Fragment>
        ))}
      </View>
      <PairDesktopButton
        label={props.pairLabel}
        onPress={props.onPairDesktop}
      />
    </ScrollView>
  );
}
