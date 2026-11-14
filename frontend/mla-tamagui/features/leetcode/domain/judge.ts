/**
 * LeetCode judge response types.
 *
 * Ported from the reference LeetCodeClient. In this app the HTTP calls run as
 * same-origin fetches inside the authenticated WebView bridge (see
 * data/webBridgeClient.ts); these types describe the JSON the judge
 * returns through that bridge.
 *
 * Moved here from the former `services/leetcode/types.ts` so the leetcode
 * feature owns its own domain contracts.
 */

export type PollState = 'PENDING' | 'STARTED' | 'SUCCESS' | 'FAILURE';

export type InterpretResponse = {
  interpret_id: string;
  interpret_expected_id?: string;
};

export type SubmitResponse = {
  submission_id: string;
};

export type PollResult = {
  state: PollState;
  run_success?: boolean;
  status_code?: number;
  status_msg?: string;
  status_runtime?: string;
  code_answer?: string[];
  expected_code_answer?: string[];
  std_output_list?: string[];
  total_correct?: number;
  total_testcases?: number;
  compile_error?: string;
  full_compile_error?: string;
  runtime_error?: string;
};
