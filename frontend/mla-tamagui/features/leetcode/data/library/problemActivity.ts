import type { Submission } from '../../domain/library/library';

const submission = (id: number, runtime?: string, memory?: string): Submission => ({
  id: String(id),
  status: runtime ? 'Accepted' : 'Runtime Error',
  language: 'Python3',
  date: id <= 3 ? 'Nov 16, 2026' : id === 8 ? 'Apr 11, 2026' : 'May 29, 2026',
  runtime,
  memory,
});

export const sampleSubmissions = [
  submission(1, '0 ms', '20.5 MB'), submission(2, '0 ms', '20.3 MB'), submission(3, '1741 ms', '19.7 MB'),
  submission(4), submission(5), submission(6, '3 ms', '20.6 MB'), submission(7, '3 ms', '20.4 MB'),
  submission(8), submission(9, '0 ms', '20.4 MB'),
];

export const sampleSubmissionCode = `class Solution:
    def twoSum(self, nums: list[int], target: int) -> list[int]:
        num_idx = dict()
        n = len(nums)
        for i in range(n):
            num = nums[i]
            comp = target - num
            if comp in num_idx:
                return [i, num_idx[comp]]
            num_idx[num] = i
        return [-1, -1]`;

export const sampleSolutions = [
  { id: 'editorial', author: 'LeetCode', title: 'Two Sum', summary: 'Approach 1: Brute Force Algorithm. Loop through the array and find another value that equals the complement.', language: 'Editorial', metrics: '5.1K   ◉ 14.6M   ▤ 2.8K' },
  { id: 'walkthrough', author: 'nits', title: '[Video] Step by Step Easy Solution', summary: 'Use a HashMap to look up the indices while traversing the array.', language: 'Python3', metrics: '334   ◉ 12.1K   ▤ 11' },
  { id: 'methods', author: 'Rahul Varma', title: '✅ 3 Methods! C++ | JAVA | PYTHON | Beginner Friendly 🔥', summary: 'Compare the brute force, two-pass hash table, and one-pass hash table methods.', language: 'Java', metrics: '12.3K   ◉ 2.4M   ▤ 293' },
  { id: 'hash-map', author: 'nits', title: 'Hash Map', summary: 'Store the index of each value and look for the target minus the current value.', language: 'Python3', metrics: '4.5K   ◉ 257.9K   ▤ 15' },
];

export const companies = [
  ['Google', '3,222', '#4285f4'], ['Amazon', '2,023', '#ff9900'], ['Meta', '1,391', '#0866ff'],
  ['Microsoft', '1,309', '#68b82e'], ['Bloomberg', '1,222', '#633be7'], ['TCS', '263', '#9e496d'],
  ['Infosys', '246', '#007cc3'], ['Apple', '279', '#949499'], ['Wipro', '18', '#824ad5'],
  ['Oracle', '360', '#e22929'], ['Accenture', '99', '#aa43f2'], ['HCL', '26', '#076a94'],
  ['MicroStrategy', '8', '#c63448'], ['Citadel', '7', '#368f8f'],
] as const;

export const twoSumHints = [
  'A really brute force way would be to search for all possible pairs of numbers but that would be too slow. Again, it’s best to try out brute force solutions just for completeness. It is from these brute force solutions that you can come up with optimizations.',
  'If we fix one of the numbers, say x, we have to scan the entire array to find the next number y which is value − x where value is the input parameter. Can we change our array somehow so that this search becomes faster?',
  'The second train of thought is, without changing the array, can we use additional space somehow? Like maybe a hash map to speed up the search?',
];
