/**
 * The dashboard's shared 3-level drill-down surface, reused by every chart
 * and list on the screen (error trend, service uptime, log levels, recent
 * logs, recent traces, services grid) so the interaction model is identical
 * regardless of which data category the user is exploring:
 *
 *   Level 1 — tapping a chart bar/slice/point (or a list row) opens this
 *             dialog showing a filtered LIST of `DrillItem`s.
 *   Level 2 — tapping a row in that list replaces the list with a DETAIL
 *             view of that single item's formatted fields.
 *   Level 3 — tapping "View raw JSON" in the detail view reveals the item's
 *             complete, unformatted underlying data.
 *
 * One dialog with internal drill state, rather than stacking multiple
 * Sheet/Dialog primitives, keeps this reliable on web (gorhom's bottom sheet
 * has documented web-only gesture quirks — see Sheet.tsx's own comments) and
 * keeps every entry point consistent.
 */

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  Badge,
  type BadgeProps,
  Button,
  ListItem,
  Text,
  View,
  useOpenSheetStore,
} from '@lib/tamagui-design-system';
import * as React from 'react';
import { ScrollView } from 'react-native';

export type DrillItem = {
  id: string;
  title: string;
  subtitle?: string;
  badge?: { label: string; variant: BadgeProps['variant'] };
  /** Full field set shown in the level-2 detail view and level-3 raw view. */
  raw: Record<string, string | number | boolean>;
};

export type DrillState = {
  title: string;
  items: DrillItem[];
  selected?: DrillItem;
  showRaw?: boolean;
} | null;

type DrillDialogProps = {
  state: DrillState;
  onClose: () => void;
  onSelect: (item: DrillItem) => void;
  onShowRaw: () => void;
  onBack: () => void;
};

function DrillDialogList({
  items,
  onSelect,
}: {
  items: DrillItem[];
  onSelect: (item: DrillItem) => void;
}) {
  if (items.length === 0) {
    return (
      <Text variant="muted" testID="ServiceReportScreen-DrillDialog-Empty">
        Nothing to show.
      </Text>
    );
  }
  return (
    <View gap="$2" testID="ServiceReportScreen-DrillDialog-List">
      {items.map((item) => (
        <ListItem
          key={item.id}
          onPress={() => onSelect(item)}
          testID={`ServiceReportScreen-DrillDialog-Item-${item.id}`}
        >
          <View
            gap="$2"
            testID={`ServiceReportScreen-DrillDialog-Item-${item.id}-Row`}
          >
            <View
              gap="$0.5"
              testID={`ServiceReportScreen-DrillDialog-Item-${item.id}-Info`}
            >
              <Text
                variant="strong"
                testID={`ServiceReportScreen-DrillDialog-Item-${item.id}-Title`}
              >
                {item.title}
              </Text>
              {item.subtitle ? (
                <Text
                  variant="muted"
                  testID={`ServiceReportScreen-DrillDialog-Item-${item.id}-Subtitle`}
                >
                  {item.subtitle}
                </Text>
              ) : null}
            </View>
            {item.badge ? (
              <Badge
                variant={item.badge.variant}
                testID={`ServiceReportScreen-DrillDialog-Item-${item.id}-Badge`}
              >
                {item.badge.label}
              </Badge>
            ) : null}
          </View>
        </ListItem>
      ))}
    </View>
  );
}

function DrillDialogDetail({ item }: { item: DrillItem }) {
  return (
    <View gap="$3" testID="ServiceReportScreen-DrillDialog-Detail">
      {item.badge ? (
        <Badge
          variant={item.badge.variant}
          testID="ServiceReportScreen-DrillDialog-Detail-Badge"
        >
          {item.badge.label}
        </Badge>
      ) : null}
      {Object.entries(item.raw).map(([key, value]) => (
        <View
          key={key}
          gap="$0.5"
          testID={`ServiceReportScreen-DrillDialog-Detail-Field-${key}`}
        >
          <Text
            variant="muted"
            testID={`ServiceReportScreen-DrillDialog-Detail-Field-${key}-Label`}
          >
            {key}
          </Text>
          <Text
            testID={`ServiceReportScreen-DrillDialog-Detail-Field-${key}-Value`}
          >
            {String(value)}
          </Text>
        </View>
      ))}
    </View>
  );
}

function DrillDialogRaw({ item }: { item: DrillItem }) {
  return (
    <Text variant="small" testID="ServiceReportScreen-DrillDialog-Raw">
      {JSON.stringify(item.raw, null, 2)}
    </Text>
  );
}

function DrillDialog({
  state,
  onClose,
  onSelect,
  onShowRaw,
  onBack,
}: DrillDialogProps) {
  const selected = state?.selected;
  const { open, close } = useOpenSheetStore();

  React.useEffect(() => {
    if (state !== null) {
      open();
    } else {
      close();
    }
  }, [state, open, close]);

  return (
    <AlertDialog
      open={state !== null}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <AlertDialogContent testID="ServiceReportScreen-DrillDialog-Content">
        <AlertDialogHeader>
          <AlertDialogTitle testID="ServiceReportScreen-DrillDialog-Title">
            {selected ? selected.title : (state?.title ?? '')}
          </AlertDialogTitle>
          {selected?.subtitle ? (
            <AlertDialogDescription testID="ServiceReportScreen-DrillDialog-Subtitle">
              {selected.subtitle}
            </AlertDialogDescription>
          ) : null}
        </AlertDialogHeader>

        <ScrollView
          style={{ maxHeight: 360 }}
          testID="ServiceReportScreen-DrillDialog-Scroll"
        >
          {!selected ? (
            <DrillDialogList items={state?.items ?? []} onSelect={onSelect} />
          ) : !state?.showRaw ? (
            <DrillDialogDetail item={selected} />
          ) : (
            <DrillDialogRaw item={selected} />
          )}
        </ScrollView>

        <AlertDialogFooter>
          {selected && !state?.showRaw ? (
            <Button
              variant="outline"
              onPress={onShowRaw}
              testID="ServiceReportScreen-DrillDialog-ShowRaw"
            >
              View raw JSON
            </Button>
          ) : null}
          {selected ? (
            <Button
              variant="secondary"
              onPress={onBack}
              testID="ServiceReportScreen-DrillDialog-Back"
            >
              Back
            </Button>
          ) : null}
          <AlertDialogAction
            onPress={onClose}
            testID="ServiceReportScreen-DrillDialog-Close"
          >
            Close
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

export { DrillDialog };
