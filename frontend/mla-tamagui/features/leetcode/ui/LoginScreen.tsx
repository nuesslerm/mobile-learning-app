import { Logger } from '@lib/logging';
import { Spinner, Text, View } from '@lib/tamagui-design-system';
import { useFocusEffect } from 'expo-router';
import { useCallback, useEffect, useState } from 'react';
import { YStack } from '@lib/tamagui-design-system/primitives';

import { useLeetCodePorts } from '../di/LeetCodeContainer';
import { useLeetCodeAuthStore } from '../state/authStore';

import { deriveCaptureBannerMessage } from './captureBannerMessage';
import { deriveLoginStatusMessage } from './loginStatusMessage';

const CAPTURE_STATUS_POLL_MS = 3000;

/**
 * Read-only connection status for leetcode-backend. There is no login form
 * here on either platform: signing in is an out-of-band operator action (a
 * separate login-capture tool drives a real browser and pushes the resulting
 * session straight to the backend). This screen only reflects what the
 * backend currently reports.
 */
export function LoginScreen(): React.ReactElement {
  const setStatus = useLeetCodeAuthStore((s) => s.setStatus);
  const { gateway } = useLeetCodePorts();
  const [loading, setLoading] = useState(true);
  const [message, setMessage] = useState(() =>
    deriveLoginStatusMessage({ isSignedIn: false, username: null })
  );
  const [captureBanner, setCaptureBanner] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const result = await gateway.userStatus();
      Logger.info('leetcode.login.status_loaded', {
        isSignedIn: result.isSignedIn,
        hasUsername: result.username != null,
      });
      setStatus(result.isSignedIn, result.username);
      setMessage(deriveLoginStatusMessage(result));
    } catch (err) {
      Logger.withError(err).warn('leetcode.login.status_load_failed');
      setStatus(false, null);
      setMessage(
        deriveLoginStatusMessage({ isSignedIn: false, username: null })
      );
    } finally {
      setLoading(false);
    }
  }, [gateway, setStatus]);

  useFocusEffect(
    useCallback(() => {
      void refresh();
    }, [refresh])
  );

  useEffect(() => {
    let cancelled = false;

    const pollCaptureStatus = async () => {
      try {
        const status = await gateway.loginCaptureStatus();
        if (!cancelled)
          setCaptureBanner(deriveCaptureBannerMessage(status, Date.now()));
      } catch (err) {
        Logger.withError(err).warn('leetcode.login.capture_status_poll_failed');
      }
    };

    void pollCaptureStatus();
    const intervalId = setInterval(() => {
      void pollCaptureStatus();
    }, CAPTURE_STATUS_POLL_MS);

    return () => {
      cancelled = true;
      clearInterval(intervalId);
    };
  }, [gateway]);

  return (
    <View flex={1} bg="$background" testID="LoginScreen">
      <YStack gap="$1" px="$4" pt="$3" pb="$2" testID="LoginScreenStatus">
        <Text variant="strong" testID="LoginScreenMessage">
          {message}
        </Text>
        <Text variant="muted" testID="LoginScreenExplainer">
          Signing in happens out-of-band, run by an operator via a separate
          login-capture session. This screen only shows whether leetcode-backend
          currently has a valid LeetCode session.
        </Text>
        {captureBanner ? (
          <Text
            variant="small"
            color="$yellow10"
            testID="LoginScreenCaptureBanner"
          >
            {captureBanner}
          </Text>
        ) : null}
      </YStack>

      {loading ? (
        <YStack
          flex={1}
          items="center"
          justify="center"
          testID="LoginScreenSpinner"
        >
          <Spinner size="large" />
        </YStack>
      ) : null}
    </View>
  );
}
