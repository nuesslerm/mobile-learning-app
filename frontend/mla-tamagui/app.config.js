const { withAndroidManifest } = require('expo/config-plugins');
const pkg = require('./package.json');

/** Direct accessor for dw tooling, mirroring mobile-learning-app. */
const BUNDLE_ID = 'com.nuesslerm.mla-tamagui';
/** Direct accessor for dw tooling. */
const ANDROID_PACKAGE = 'com.nuesslerm.mla_tamagui';

/**
 * The Tamagui sibling of `mobile-learning-app` -- same feature target, a
 * different design system, so the two can be compared side by side.
 *
 * `slug` and `experiments.baseUrl` must match wf's frontend app table
 * (`clis/deploy/src/ship_eas.rs`): a wrong slug yields an
 * exp:// link that opens nothing, and a wrong baseUrl yields a deploy that
 * 404s on every asset, both silently.
 *
 * @param {import('@expo/config-types').ExpoConfig} _config
 * @returns {{ expo: import('@expo/config-types').ExpoConfig }}
 */
// The lab APK reaches the backends by tailnet IP over plain HTTP; production builds keep Android's cleartext block.
const withCleartextLabTraffic = (config) =>
  withAndroidManifest(config, (c) => {
    c.modResults.manifest.application[0].$['android:usesCleartextTraffic'] = 'true';
    return c;
  });

function expoConfig(_config) {
  return {
    expo: {
      name: 'mla-tamagui',
      slug: 'mla-tamagui',
      version: pkg.version,
      orientation: 'portrait',
      icon: './assets/images/icon.png',
      scheme: 'mla-tamagui',
      userInterfaceStyle: 'automatic',
      newArchEnabled: true,
      ios: {
        supportsTablet: true,
        bundleIdentifier: BUNDLE_ID,
      },
      android: {
        package: ANDROID_PACKAGE,
        edgeToEdgeEnabled: true,
      },
      web: {
        bundler: 'metro',
        output: 'static',
        favicon: './assets/images/favicon.png',
      },
      plugins: [
        'expo-router',
        // Without it a dev or EAS build has no NSMicrophoneUsageDescription: iOS closes the app the first time the
        // voice editor records. Expo Go supplies its own string, which is why nothing failed there.
        [
          'expo-audio',
          { microphonePermission: 'Allow $(PRODUCT_NAME) to record the instructions you speak to edit code.' },
        ],
        [
          'expo-splash-screen',
          {
            image: './assets/images/splash-icon.png',
            imageWidth: 200,
            resizeMode: 'contain',
            backgroundColor: '#ffffff',
          },
        ],
      ],
      experiments: {
        typedRoutes: true,
        baseUrl: '/mla-tamagui',
      },
      extra: {
        router: {},
        eas: {
          projectId: '5d273b55-a8fc-4e4a-bae7-14d1a9f7eb57',
        },
      },
      owner: 'nuesslerm',
      updates: {
        url: 'https://u.expo.dev/5d273b55-a8fc-4e4a-bae7-14d1a9f7eb57',
      },
      // Matches the SDK of Expo Go (57), and it is what makes an update loadable in Expo Go
      // rather than only in a custom dev client.
      runtimeVersion: 'exposdk:57.0.0',
    },
  };
}

module.exports = (config) => {
  const { expo } = expoConfig(config);
  return process.env.EXPO_PUBLIC_ANDROID_CLEARTEXT === '1' ? withCleartextLabTraffic(expo) : { expo };
};
Object.assign(module.exports, { BUNDLE_ID });
Object.assign(module.exports, { ANDROID_PACKAGE });
