import { LeetCodeLiveContainer } from '../di/LeetCodeContainer';
import { LeetCodeFixtureOverrides } from '../di/fixtureOverrides';
import { LibraryHomeScreen } from '../ui/library/LibraryHomeScreen';

export default function AlgorithmsLibraryRoute() {
  return (
    <LeetCodeFixtureOverrides>
      <LeetCodeLiveContainer>
        <LibraryHomeScreen />
      </LeetCodeLiveContainer>
    </LeetCodeFixtureOverrides>
  );
}
