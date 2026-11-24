import { LeetCodeLiveContainer } from '../di/LeetCodeContainer';
import { LeetCodeFixtureOverrides } from '../di/fixtureOverrides';
import { AlgorithmsLibraryScreen } from '../ui/library/AlgorithmsLibraryScreen';

export default function AlgorithmsLibraryRoute() {
  return (
    <LeetCodeFixtureOverrides>
      <LeetCodeLiveContainer>
        <AlgorithmsLibraryScreen />
      </LeetCodeLiveContainer>
    </LeetCodeFixtureOverrides>
  );
}
