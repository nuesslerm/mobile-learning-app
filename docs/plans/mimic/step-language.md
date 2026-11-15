# The `wf mimic` step language and interpreter

One interpreter runs a case's `script` for both the reference (recording) and the candidate (replay); only the `Policy` differs. It lives in
the versioned library `mimic-script-core` (`clis/mimic/script/core`), a pure core behind `Transport`, `Target` and `Clock` ports, with its
adapters as sibling libraries. Issue #83 step 1 landed the core and the T3 dialect; the Orca dialect (section 3) arrives with step 4. Proof of
the T3 dialect: `core/tests/flow.rs` runs the 1,712 committed recorded, crawl and scenario scripts of the T3 sets against their own recorded
steps (`CassetteEchoTransport`) and every one reproduces its cassette steps.

Written by the design workflow of 15 Nov 2026; where the code differs, the code and its tests are the source.

## 0. Scope and decisions (resolving the readers' contradictions)

One interpreter in Rust runs the `script` op for BOTH recording (reference = real T3 / real orcad) and replay (candidate = t3-serve / orca-serve). Reference and candidate run identical code; the only difference is the `Policy` value (section 5). The op name stays `script` (clis/mimic/core/src/t3_golden.rs:18), so every committed cassette input stays valid.

Drift decisions (TS = tooling/mimic-drivers/src/t3/client.ts, RS = clis/t3-serve/tests/flow/support/mimic_t3.rs):
- Op key: the entry must have exactly one key that is not `await`/`capture`; that key is the op. 0 or 2+ such keys is a request error. (TS takes the first key, client.ts:56; RS skips only `await`, mimic_t3.rs:213, which breaks on key-sorted cassette files carrying `capture`.)
- await order: TS order. After a ws-send with `await:"exit"`: wait for the Exit, THEN quiet window. (RS settled first, mimic_t3.rs:247-254.)
- Plain ws-send of a `_tag:"Request"` frame without await: wait up to `first_answer_ms` for the first s2c frame, then quiet window (RS behaviour, adopted for both sides; TS had none). Other frames: quiet window only.
- Exit match: `String(requestId) == String(sent.id)` (not serde Value equality).
- No busy spin: all waits go through `Clock::sleep`; with no socket the wait ends at once.
- Transport failures are recorded behaviour: a `driver` step `{transport_error}` plus outcome `{status:"error",code:"transport"}` (TS), never a DriverError.
- ws-open records the real HTTP status: 101 on success. A refused upgrade records `ws:open {path,status:<real>}` and leaves no socket; later ws steps then end in transport_error `no socket`. (TS hard-coded 101 and failed; no cassette has a non-101, so cassettes are unaffected.)
- ws-close always pushes the final synthetic `s2c ws:close {code:null,reason:"",tcp:true}` (TS client.ts:105-110), also when the peer close was not seen within `close_wait_ms`.
- Server-initiated close or drop mid-script is recorded as an `s2c ws:close` step at its arrival position (RS behaviour; TS only produced transport_error). Unexercised by cassettes except one 1006 crawl row; verify in migration step 1.
- Form body encoding follows URLSearchParams (space -> `+`, `~` -> `%7E`, values `String()`'d). Response headers: names lowercased, duplicate names joined with `, `.
- Frames arriving while an http call is in flight are recorded BEFORE that call's call/ret pair (TS async order). The Transport adapter reads the socket on a reader thread into a queue; the core drains the queue at every step boundary and during every wait.
- Concurrency: the interpreter runs cases serially (`run_many` = default sequential). No MIMIC_DRIVER_CONCURRENCY. Parallelism is a later optimisation (one server per worker thread).

## 1. Case and entry shape

Case input (JSON, unchanged): `{script: Entry[], settle_ms?: u64 (default 250), prelude?: "session"}`. Legacy Orca input `{requests:[{method,params,timeout_ms,capture}], connect_timeout_ms?, setup:[{git}]}` is accepted and desugared to entries (section 3).
Entry = JSON object: one op key + optional `capture` (all ops) + optional `await` (`"exit"`, only meaningful on ws-send; any other value is a request error `script: unknown await X`).

## 2. T3 dialect: steps

Output step shape and key order: `{dir, channel, kind, rid?, payload}` (client.ts:29-37). Built only through `mimic_core::wire::{WireEvent, raw_steps}` (clis/mimic/core/src/wire.rs:61), never by hand in the interpreter, except `driver` steps.

| op | arg fields (defaults) | semantics | emits |
|---|---|---|---|
| `http` | `method` (GET), `path`, `headers` ({}), `body` (null) | Fill arg. Body: null -> none; string -> as is; object -> urlencoded if content-type (case-insensitive) contains `x-www-form-urlencoded` else JSON text. GET with a body is sent anyway. No redirect following. Response fully read. Auto-capture `access_token`, `ticket` (string only) from a JSON body after explicit `capture`. A failed request emits nothing for that entry (transport_error). | after the response: `call/http/json {method,path,headers,body}` (filled values, body string or null) then `ret/http/json {status,headers,body}` (body raw string, null if empty) |
| `ws-open` | `path` | URL = base with http->ws. Single socket; a second ws-open closes and replaces the first without a step. | `s2c ws:open/json {path,status}` (before any frame of this socket) |
| `ws-send` | the JSON frame itself (typically `{_tag:"Request",id,tag,payload,headers:[]}`); siblings `await`, `capture` | Fill; serialize whole filled arg; push c2s step BEFORE writing. Then wait rule (section 0): await-exit / first-answer, then quiet(settle). Then apply `capture` against the Exit frame of this send (base pointer = Exit frame, e.g. `/exit/value/runs/0/runId`); no Exit -> no capture. No socket -> transport_error. | `c2s ws/json rid? payload` for the frame; each received frame as `s2c ws/json|text rid? payload` in arrival order; each Chunk frame is followed at once by a `c2s ws/json {_tag:"Ack",requestId}` step and the Ack is sent |
| `ws-close` | `code` (1000), `reason` ("") | push c2s close, send close, wait up to `close_wait_ms` for peer close. No socket -> transport_error. No settle afterwards. Steps after it still run. | `c2s ws:close {code,reason,tcp:false}`; `s2c ws:close {code,reason,tcp:false}` if seen; ALWAYS `s2c ws:close {code:null,reason:"",tcp:true}` |
| unknown op | | request error `script: unknown script step <op>`; no Observed. | none |

Frame step: JSON text -> `kind:json`, `rid = String(payload.id ?? payload.requestId)` only if string or number; non-JSON -> `kind:text`, no rid; binary -> `kind:binary` with `len`, base64 payload (wire.rs:70-72). Ping/pong frames are not recorded.

## 3. Orca dialect (new ops, same entry grammar)

| op | fields | semantics | emits |
|---|---|---|---|
| `git` (setup) | name | Exec port: deterministic repo in scratch (fixed author/date env, one commit, branch main). Runs before connect. | none |
| `connect` | `timeout_ms` (10000), `capture_handshake` (false), `capture_close` (false), `device_token`, `server_public_key_b64` (override the host's pairing) | the phone connects (`Phone::connect_with`); state != connected -> outcome `{status:"error",code:<state>}` and NO steps (orca/client.ts:98-100), script ends. The overrides are filled like any argument (`{bad-credential}`) | with `capture_handshake`: `c2s`/`s2c` `channel:"handshake"` steps `{stage, frame}` (hello, ready, auth, authenticated, error), keys and nonces shaped, token masked; with `capture_close`: the end of the connection as `s2c channel:"ws:close"` `{code, reason}` at the end of the case |
| `request` | `method`, `params`, `timeout_ms` (10000), `rid` (index of the request among request/subscribe entries, as string), `capture` | one sealed request/response through the phone client | `c2s json {method,params}` rid; `s2c json <frame minus id>` rid; client failure -> `s2c json {client_error:msg}` |
| `subscribe` | `method`, `params`, `rid`, `listen_ms` (0) | opens a stream and listens for `listen_ms`; the stream's id is mapped to `rid` | `c2s` opener step, then every stream frame `s2c json` (`binary` with `len` for bytes) with the opener's rid, in arrival order; a refused stream is `s2c {client_error}` |
| `listen` | `ms` | keep draining the phone's events for `ms` | frames as above |
| `unsubscribe` | `rid` | the phone ends the stream (the client sends its own unsubscribe); the quiet window (`settle_ms`) takes what was still on its way | late frames of the stream |

The phone session always closes when a case ends (there is no `close` op); the end of the connection is a step only with `capture_close`.

Legacy desugar: `connect{timeout_ms: connect_timeout_ms}` + `request{rid:String(i)}` per requests[i]; so the 286 committed orca rows reproduce byte for byte. Masks (`<uuid:1>`, `<volatile:updatedAt>`, `<userdata>`, `<scratch>`) stay in the rule set; the interpreter only replaces the per-run workspace/scratch/userdata path by `<workspace>`/`<scratch>`/`<userdata>` (existing orca/client.ts:60-72 behaviour).

## 4. Variables and templating

- Static vars: `workspace` (server workspace dir), `bad-credential` = `NOTACREDENTIAL`.
- `credential`: minted lazily by `Target::mint()` only if the script JSON text contains `{credential}`; one mint per run (per generation in session mode). T3 mint = `t3 auth pairing create --json`, first string of keys credential|token|code|secret (release.ts:69-74).
- Auto-captured on every http answer with JSON body: `access_token`, `ticket` (strings). Non-JSON body: nothing.
- Explicit `capture: {name: {from: [json-pointer...], prefix?}}`: per name the first pointer whose value is a string or number wins; strings get `prefix` prepended, numbers stay numbers; no match -> var unset. Pointer: split on `/`, drop first empty, unescape `~1`,`~0`, array index by string key. Base: parsed http body, or Exit frame of the ws-send.
- fill(): a string exactly `{name}` (regex `[A-Za-z][\w.-]*`) with a bound var becomes the var value, type preserved; embedded `{name}` is replaced by the String() form; unbound names stay literal. Recurses arrays/objects.
- unbound check: names declared by any `capture` in the script that are still unset and appear in the filled arg -> entry NOT sent; step `ret/driver/json {client_error:"unbound variable <name>", op}`; script continues; outcome stays ok. Undeclared unminted names are sent literally.
- Masking of run-specific paths: after the run every string in every step payload (deep) has the workspace path replaced by `<workspace>`.

## 5. Determinism: all constants in ONE place (`script_core::policy::Policy`, `Default`)

| name | default | notes |
|---|---|---|
| settle_ms | case value, else 250 (corpus/crawl 400, turn scenarios 1500, prelude 250) | quiet window: trailing silence measured from the last frame OR last send |
| poll_ms | 25 | granularity of every wait; sleep = min(window, poll_ms) |
| first_answer_ms | 3000 | only Request frames without await |
| await_exit_ms | 15000 | on timeout continue silently (recorded as is), then quiet |
| close_wait_ms | 2000 | |
| http_timeout_ms | 30000 | expiry -> transport_error `timeout` |
| ready_ms / ready_poll_ms | 60000 / 250 | Target readiness |
| quiet_cap_ms | None | replay-only knob: candidate runs set `Some(120)` in mimic.toml; never set when recording a reference |
Rules: no wall-clock or random value ever enters a step; all time goes through `Clock`; steps are ordered by arrival at the Transport queue, ties by queue order; one mint per run; ports/paths/tempdirs never appear in steps (workspace masked); same `Case` + same Transport behaviour + `FakeClock` gives identical steps.

## 6. Errors versus outcomes

- Request-level `{id,error}` (no Observed): unknown op, bad `await`, malformed entry, mint failure, Target start/ready failure, pin/sha mismatch, prelude definition missing.
- Recorded behaviour (Observed): transport_error step (ends script) with outcome error/transport; client_error step (script continues, outcome ok); orca connect failure (no steps, outcome error/<state>).
- Outcome = error/transport iff the LAST step is a `driver` step with `transport_error`. `effects` always `[]`. Missing reference_accepts stays absent.

## 7. Prelude (`prelude:"session"`)

Prelude is DATA, not code: 5 entries stored as `prelude.json` next to the set, referenced from `[reference.script] prelude = ...` in mimic.toml (same for candidate). Entries: [0] oauth token-exchange form with `{credential}`, [1] POST /api/auth/websocket-ticket/rpc with bearer, [2] ws-open `/ws?orchestrationProtocol=2&wsTicket={ticket}`, [3] ws-send p1 projects.mutate await exit, [4] ws-send p2 orchestration.dispatchCommand await exit (contents from corpus/t3.ts:158-166). Generated once by the TS corpus code until it is retired.
- Fresh mode (`session=false`): script = prelude ++ case script.
- Session mode (default when the Target is a spawned server; `MIMIC_SESSION=fresh` disables): once per server generation run prelude ++ `ws-close` at settle 250, cut steps before the first ws:close, keep access_token; each prelude-flagged case runs entries [1],[2] with access_token preset, then its script; result steps = cached prelude steps ++ steps after the case's own ws:open step. `reset` stops the server, bumps generation, clears cache.
- Case without the flag: runs as is.

## 8. Target modes (from env/config, mirrors index.ts/release.ts)

`spawn` (pinned T3 release: sha-checked tarball into cache, empty env + PATH/HOME/T3CODE_HOME/TZ/LANG, `serve --host 127.0.0.1 --port P --no-browser <workspace>`, readiness GET /.well-known/t3/environment), `url` (existing endpoint, T3_URL + one credential, workspace from T3_WORKSPACE), `chain` (candidate: URL of the t3-serve/orca-serve chain supplied by the test harness). Pairing URIs and credentials never appear in a case or cassette.
