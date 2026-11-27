import { useSkin } from '@lib/skin';

export function OrcaBotsTab({ computer }: { computer: string | null | undefined }) {
  const { HomeTabHeader, HomeEmptyState } = useSkin();
  return (
    <>
      <HomeTabHeader title="Bots" />
      {computer !== undefined && (
        <HomeEmptyState
          icon="bots"
          title={computer === null ? 'No bots yet' : 'Update Orca on ' + computer}
          message={computer === null
            ? 'Pair a desktop to see its bots here.'
            : 'Its version of Orca can’t show bots on your phone yet.'}
        />
      )}
    </>
  );
}
