import {
  Badge,
  Button,
  IconButton,
  Text,
  useKeyboardFrame,
  type BadgeProps,
} from '@lib/tamagui-design-system';
import {
  ChevronDown,
  ChevronUp,
  Merge,
  Mic,
  Play,
  Plus,
  Scissors,
  Square,
  Trash2,
} from '@lib/tamagui-design-system/icons';
import type { EditAlternative, PlanStep } from '@lib/voice-edit-dsl';
import { useCallback, useEffect, useRef, useState } from 'react';
import { Platform, ScrollView, TextInput } from 'react-native';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { useLiveTranscript } from '../hooks/useLiveTranscript';
import { usePlanRealize } from '../hooks/usePlanRealize';
import { usePlanStore } from '../state/planStore';

export type PlanTabProps = {
  problemId: string;
  language: string;
  getCode: () => string;
  applyEdit: (alt: EditAlternative) => { modifiedText: string };
  /** False while the editor beside this tab has not handed over its API yet. */
  canRealize?: boolean;
};

const STATUS_VARIANT: Record<PlanStep['status'], BadgeProps['variant']> = {
  pending: 'secondary',
  realized: 'success',
  stale: 'warning',
};

function statusLabel(step: PlanStep): string {
  if (step.status === 'realized' && step.lines) {
    return `lines ${step.lines.startLine}\u2013${step.lines.endLine}`;
  }
  return step.status;
}

const INPUT_STYLE = {
  minHeight: 38,
  borderRadius: 8,
  borderWidth: 1,
  borderColor: '#8884',
  paddingHorizontal: 10,
  paddingVertical: 6,
  fontSize: 14,
} as const;

/**
 * Plan tab: an ordered, editable list of the problem's steps, each realisable
 * into the code buffer one at a time, plus a live area that turns speech or a
 * typed block into new steps.
 */
export function PlanTab({
  problemId,
  language,
  getCode,
  applyEdit,
  canRealize = true,
}: PlanTabProps) {
  const steps = usePlanStore((state) => state.plans[problemId]) ?? [];
  const {
    addBlock,
    setStepText,
    splitStep,
    mergeWithNext,
    moveStep,
    removeStep,
  } = usePlanStore();
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editDraft, setEditDraft] = useState('');
  const [draft, setDraft] = useState('');
  const editIdRef = useRef(`plan-${Date.now().toString(36)}`);
  const stoppedRef = useRef(false);

  const live = useLiveTranscript({ editId: editIdRef.current });
  const keyboard = useKeyboardFrame();
  // Android already reports the keyboard inset in the window frame.
  const composerInset = Platform.OS === 'android' ? 0 : keyboard.height;
  const { realize, busyStepId, error } = usePlanRealize({
    problemId,
    language,
    getCode,
    applyEdit,
  });

  useEffect(() => {
    if (!live.listening && stoppedRef.current && live.text.trim()) {
      stoppedRef.current = false;
      addBlock(problemId, live.text, 'voice');
    }
  }, [addBlock, live.listening, live.text, problemId]);

  const toggleMic = useCallback(() => {
    if (live.listening) {
      stoppedRef.current = true;
      void live.stop();
    } else {
      stoppedRef.current = false;
      void live.start();
    }
  }, [live.listening, live.start, live.stop]);

  const commitEdit = useCallback(() => {
    if (editingId) setStepText(problemId, editingId, editDraft);
    setEditingId(null);
  }, [editDraft, editingId, problemId, setStepText]);

  const handleAdd = useCallback(() => {
    if (!draft.trim()) return;
    addBlock(problemId, draft);
    setDraft('');
  }, [addBlock, draft, problemId]);

  return (
    <YStack testID="ProblemPlanTab" flex={1} width="100%">
      {canRealize ? null : (
        <Text testID="ProblemPlanRealizeHint" variant="muted" px="$3" pt="$2">
          Opening the editor…
        </Text>
      )}
      <ScrollView
        testID="ProblemPlanSteps"
        style={{ flex: 1 }}
        contentContainerStyle={{ padding: 12 }}
        keyboardShouldPersistTaps="handled"
      >
        {steps.length === 0 ? (
          <Text variant="muted">No steps yet.</Text>
        ) : (
          <YStack borderWidth={1} borderColor="$borderColor" rounded="$4">
            {steps.map((step, i) => (
              <YStack
                key={step.id}
                testID={`ProblemPlanStep-${i}`}
                gap="$1.5"
                p="$2"
                borderBottomWidth={i === steps.length - 1 ? 0 : 1}
                borderBottomColor="$borderColor"
              >
                <XStack items="center" gap="$2">
                  <Text
                    variant="muted"
                    width={20}
                    style={{ textAlign: 'center' }}
                  >
                    {i + 1}
                  </Text>
                  {editingId === step.id ? (
                    <TextInput
                      testID={`ProblemPlanStep-${i}-Text`}
                      value={editDraft}
                      onChangeText={setEditDraft}
                      onBlur={commitEdit}
                      autoFocus
                      style={{ ...INPUT_STYLE, flex: 1 }}
                    />
                  ) : (
                    <Text
                      testID={`ProblemPlanStep-${i}-Text`}
                      flex={1}
                      onPress={() => {
                        setEditingId(step.id);
                        setEditDraft(step.text);
                      }}
                    >
                      {step.text}
                    </Text>
                  )}
                  <Badge
                    testID={`ProblemPlanStep-${i}-Status`}
                    variant={STATUS_VARIANT[step.status]}
                  >
                    {statusLabel(step)}
                  </Badge>
                </XStack>
                <XStack gap="$1.5" pl={28} flexWrap="wrap">
                  <IconButton
                    testID={`ProblemPlanStep-${i}-Realize`}
                    icon={Play}
                    aria-label={`Realize step ${i + 1}`}
                    disabled={busyStepId !== null || !canRealize}
                    onPress={() => void realize(step.id)}
                  />
                  <IconButton
                    testID={`ProblemPlanStep-${i}-Split`}
                    icon={Scissors}
                    aria-label={`Split step ${i + 1}`}
                    onPress={() => splitStep(problemId, step.id)}
                  />
                  <IconButton
                    testID={`ProblemPlanStep-${i}-Merge`}
                    icon={Merge}
                    aria-label={`Merge step ${i + 1} with next`}
                    disabled={i === steps.length - 1}
                    onPress={() => mergeWithNext(problemId, step.id)}
                  />
                  <IconButton
                    testID={`ProblemPlanStep-${i}-Up`}
                    icon={ChevronUp}
                    aria-label={`Move step ${i + 1} up`}
                    disabled={i === 0}
                    onPress={() => moveStep(problemId, step.id, -1)}
                  />
                  <IconButton
                    testID={`ProblemPlanStep-${i}-Down`}
                    icon={ChevronDown}
                    aria-label={`Move step ${i + 1} down`}
                    disabled={i === steps.length - 1}
                    onPress={() => moveStep(problemId, step.id, 1)}
                  />
                  <IconButton
                    testID={`ProblemPlanStep-${i}-Delete`}
                    icon={Trash2}
                    aria-label={`Delete step ${i + 1}`}
                    onPress={() => removeStep(problemId, step.id)}
                  />
                </XStack>
              </YStack>
            ))}
          </YStack>
        )}
      </ScrollView>

      <YStack
        testID="ProblemPlanComposer"
        gap="$2"
        px="$3"
        pt="$2"
        pb={composerInset + 12}
        borderTopWidth={1}
        borderTopColor="$borderColor"
      >
        <XStack items="center" gap="$2">
          <IconButton
            testID="ProblemPlanMicButton"
            icon={live.listening ? Square : Mic}
            aria-label={live.listening ? 'Stop listening' : 'Start listening'}
            onPress={toggleMic}
          />
          {live.listening ? (
            <Text testID="ProblemPlanLiveText" flex={1} variant="muted">
              {live.text}
            </Text>
          ) : null}
        </XStack>

        <XStack items="center" gap="$2">
          <TextInput
            testID="ProblemPlanTextInput"
            value={draft}
            onChangeText={setDraft}
            placeholder="Add a step"
            style={{ ...INPUT_STYLE, flex: 1 }}
          />
          <Button
            testID="ProblemPlanAddButton"
            icon={Plus}
            disabled={!draft.trim()}
            pressLog="off"
            onPress={handleAdd}
          >
            Add
          </Button>
        </XStack>

        {error ? (
          <Text testID="ProblemPlanError" color="$red10" variant="small">
            {error}
          </Text>
        ) : null}
      </YStack>
    </YStack>
  );
}
