# Recordings of Codex's app-server

The four `.ndjson` files are copies of wire transcripts that the T3 Code project recorded from real runs of `codex app-server`
(`apps/server/src/orchestration-v2/testkit/fixtures/<name>/codex_transcript.ndjson` of `references/t3code`, MIT licence, Copyright (c) 2026 T3 Tools Inc.).
Each line is `transcript_start`, `expect_outbound` (what the client sent) or `emit_inbound` (what the server said) with the frame; the header names
the Codex version and the scenario.

| file | Codex | what it shows |
| --- | --- | --- |
| `command_output_streaming` | 0.157.0 | a message that streams, a command that runs for seconds with output deltas, a final answer |
| `tool_call_read_only_on_request` | 0.156.1 | a command that asks for approval (`item/commandExecution/requestApproval`, answered `accept`) before it writes a file |
| `message_steering` | 0.156.1 | `turn/steer` right after the turn started: the same turn says the second answer |
| `turn_interrupt` | 0.156.1 | `turn/interrupt` and a `turn/completed` that is `interrupted` |

They were recorded by another client, not by this service: they show what the server says to the requests that client sent (it sets `experimentalApi`),
so a request of ours that the real server would refuse is not caught by them (the gap `codex-live-session-not-run-against-the-real-cli`).
The scripted `codex app-server` plays them (`exec_testkit::app_server`): it checks the method of each message the client sends against the recording,
and answers each request under the id the client gave it.
