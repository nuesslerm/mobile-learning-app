import { DependencyOverridesProvider } from '@lib/app-di';
import { renderToString } from 'react-dom/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  AudioRecordingPort,
  LeetCodeGatewayPort,
  LeetCodePorts,
  VoiceEditTransportPort,
} from '../../contracts/ports';
import {
  createDefaultContainer,
  LeetCodeContainerProvider,
  LeetCodeLiveContainer,
  useLeetCodePorts,
} from '../LeetCodeContainer';

const { expoAudioPort, useExpoAudioRecordingPortMock } = vi.hoisted(() => {
  const port: AudioRecordingPort = {
    requestPermission: vi.fn(async () => true),
    start: vi.fn(async () => {}),
    stop: vi.fn(async () => null),
    subscribe: vi.fn(() => () => {}),
  };
  return { expoAudioPort: port, useExpoAudioRecordingPortMock: vi.fn(() => port) };
});

// The Expo adapter pulls expo-audio/expo, which cannot load in the node unit
// environment; mock it so the default audio is observable without a provider.
vi.mock('../../data/ExpoAudioRecorderAdapter', () => ({
  useExpoAudioRecordingPort: useExpoAudioRecordingPortMock,
}));

describe('createDefaultContainer', () => {
  beforeEach(() => {
    useExpoAudioRecordingPortMock.mockClear();
  });

  it('Given an override for every port, When createDefaultContainer called, Then returns the overrides verbatim', () => {
    const fakeGateway = {} as LeetCodeGatewayPort;
    const fakeVoice = {} as VoiceEditTransportPort;
    const fakeAudio = {} as AudioRecordingPort;

    const ports = createDefaultContainer({
      gateway: fakeGateway,
      voice: fakeVoice,
      audio: fakeAudio,
    });

    expect(ports.gateway).toBe(fakeGateway);
    expect(ports.voice).toBe(fakeVoice);
    expect(ports.audio).toBe(fakeAudio);
    expect(useExpoAudioRecordingPortMock).not.toHaveBeenCalled();
  });

  it('Given only an audio override, When createDefaultContainer called, Then the override wins over the Expo adapter', () => {
    const fakeAudio = {} as AudioRecordingPort;

    const ports = createDefaultContainer({ audio: fakeAudio });

    expect(ports.audio).toBe(fakeAudio);
    expect(useExpoAudioRecordingPortMock).not.toHaveBeenCalled();
    expect(ports.gateway.userStatus).toBeTypeOf('function');
    expect(ports.voice.transcribe).toBeTypeOf('function');
  });

  it('Given only an audio port, When createDefaultContainer called, Then live adapters back gateway and voice and no hook runs', () => {
    const ports = createDefaultContainer({ audio: expoAudioPort });

    expect(useExpoAudioRecordingPortMock).not.toHaveBeenCalled();
    expect(ports.audio).toBe(expoAudioPort);
    expect(ports.gateway.userStatus).toBeTypeOf('function');
    expect(ports.voice.transcribe).toBeTypeOf('function');
  });

  it('Given LeetCodeLiveContainer, When a child reads the ports, Then audio comes from the Expo adapter hook', () => {
    let seen: AudioRecordingPort | undefined;
    function Probe() {
      seen = useLeetCodePorts().audio;
      return null;
    }

    renderToString(
      <LeetCodeLiveContainer>
        <Probe />
      </LeetCodeLiveContainer>
    );

    expect(useExpoAudioRecordingPortMock).toHaveBeenCalledTimes(1);
    expect(seen).toBe(expoAudioPort);
  });

  it('Given DependencyOverridesProvider with a fake gateway and audio, When LeetCodeLiveContainer renders, Then the overrides reach useLeetCodePorts and voice stays live', () => {
    const fakeGateway = {} as LeetCodeGatewayPort;
    const fakeAudio = {} as AudioRecordingPort;
    let seen: LeetCodePorts | undefined;
    function Probe() {
      seen = useLeetCodePorts();
      return null;
    }

    renderToString(
      <DependencyOverridesProvider
        overrides={{ leetcode: { gateway: fakeGateway, audio: fakeAudio } }}
      >
        <LeetCodeLiveContainer>
          <Probe />
        </LeetCodeLiveContainer>
      </DependencyOverridesProvider>
    );

    expect(seen?.gateway).toBe(fakeGateway);
    expect(seen?.audio).toBe(fakeAudio);
    expect(seen?.voice.transcribe).toBeTypeOf('function');
  });
});

describe('useLeetCodePorts', () => {
  it('Given no provider, When the hook renders, Then it throws the pinned message', () => {
    function Probe() {
      useLeetCodePorts();
      return null;
    }

    expect(() => renderToString(<Probe />)).toThrow(
      'LeetCodeContainerProvider missing'
    );
  });

  it('Given a provider with a value, When the hook renders, Then it returns that value', () => {
    const fakeAudio = {} as AudioRecordingPort;
    const ports = createDefaultContainer({
      gateway: {} as LeetCodeGatewayPort,
      voice: {} as VoiceEditTransportPort,
      audio: fakeAudio,
    });

    function Probe() {
      const consumed = useLeetCodePorts();
      return <div>{consumed.audio === fakeAudio ? 'audio-ok' : 'audio-bad'}</div>;
    }

    const html = renderToString(
      <LeetCodeContainerProvider value={ports}>
        <Probe />
      </LeetCodeContainerProvider>
    );

    expect(html).toContain('audio-ok');
  });
});