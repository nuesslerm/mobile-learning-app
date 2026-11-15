# The Orca phone sidecar protocol (`mimic-orca-phone/1`)

Design of the 15 Nov 2026 design workflow for issue #83, kept here so the implementation can follow it. The step language itself is in
`step-language.md` (section 3 names the Orca ops); this file is the wire between the Rust interpreter and the Node sidecar around the phone's
own `DirectRpcClient`.

**State of the implementation.** `tooling/mimic-drivers/src/orca/sidecar.ts` and `SidecarPhone` (`mimic-script-net`) implement a subset: strictly
request-and-reply lines without `id`, the ops `connect`, `request`, `disconnect` and `close`, and no events. It is enough for the committed
request/response rows. Streams, taps and the handshake need unsolicited events, which a read-one-line-per-call client cannot take, so the
stream step moves both sides to the protocol below (ids on control lines, a reader that queues events).

Name `mimic-orca-phone/1`: JSON lines over stdio, child of the Rust `Phone` adapter (script-orca). Stdout is protocol only; the phone bundle's console output stays redirected to stderr (phone.ts:55-57). The sidecar owns only wire facts (pinned DirectRpcClient, e2ee v2, decryption, frame tap). The Rust interpreter owns every decision: timeouts, windows, capture/fill, ordering, masking, step shaping. No `if recording` branch exists anywhere, so reference and candidate run the same sidecar command (as clis/orca-serve/cassettes/orca-files/mimic.toml already does).

Target selection is environment only (never in a case or cassette; target.ts:1-3): ORCA_PAIRING_URI (from `orca-serve pair`, clis/orca-serve/src/verbs.rs:92-111) or ORCA_ENDPOINT + ORCA_DEVICE_TOKEN + ORCA_SERVER_PUBLIC_KEY_B64 or ORCA_ORCAD_CHECKOUT (provision orcad at the pin, `orca_server_ready` line carries pairing.url).

Control lines (request/reply, matched by id):
- `{"hello":true}` -> `{"hello":{"name":"mimic-orca-phone","protocol":"mimic-orca-phone/1","target_version":"<readPin().version>","toolchain_lock_hash":"<h>"}}`. The pin is asserted at load (assertAtPin).
- `{"reset":true}` -> `{"reset":"ok"}`: drop the client; next connect builds a fresh DirectRpcClient and fresh AsyncStorage stub (clears the v2 host latch).
- `{"id","op":"tap","input":{"app":bool,"handshake":bool,"close":bool}}` -> `{"id","ok":{}}`; must precede connect; defaults all false except app (so legacy rows add no events beyond the request/response pair).
- `{"id","op":"connect","input":{"timeout_ms":10000}}` -> `{"id","ok":{"state":"connected|auth-failed|disconnected|timeout"}}`. Waits for state connected or a FAILED state (client.ts:15-33).
- `{"id","op":"request","input":{"rid":"0","method":"...","params":<json|null>,"timeout_ms":10000}}` -> `{"id","ok":{"response":<frame minus id>}}` or `{"id","ok":{"client_error":"<msg>"}}`.
- `{"id","op":"subscribe","input":{"rid":"s0","method":"...","params":<json|null>}}` -> `{"id","ok":{"sub":"s0"}}` at once. The sidecar registers the listener via `client.subscribe(method, params, onData, {onBinaryFrame, envelope})` and keeps the dispose fn.
- `{"id","op":"unsubscribe","input":{"sub":"s0"}}` -> `{"id","ok":{}}`: calls the dispose fn (the client itself picks and sends the method-specific unsubscribe: terminal.unsubscribe with requestId, session.tabs.unsubscribe, agentSession.unsubscribe, nativeChat.unsubscribe, `<x>.unsubscribe {subscriptionId}` once `ready` named the id). The c2s frame it sends is reported as an event, not authored by the interpreter. An app-level ack is a plain `request`, no separate op.
- `{"id","op":"close"}` -> `{"id","ok":{"state":"disconnected"}}` after the final close event is emitted.
- Failure that prevents an op: `{"id","error":"<msg>"}` (request-level, as mimic-driver/1).

Unsolicited events (no id), one per line, in arrival order; `seq` is a monotonic counter and `t_ms` ms since connect, both WIRE-ONLY (interpreter orders and windows with them; they never reach a cassette):
- `{"event":"frame","sub":"s0"|null,"rid":"0","seq":n,"t_ms":n,"dir":"c2s|s2c","kind":"json|binary","payload":<json | "<base64>">,"len"?:n}`: decrypted application frames. c2s payload `{method,params}` with id/deviceToken stripped; s2c payload is the frame without `id`, keeping `_meta`, `ok`, `result`/`error`, `streaming:true`. Stream chunks share the opener's rid. Tap points: wrap the sealText/openText/openBinary pair of the e2ee v2 client session (mobile-e2ee-v2-physical-channel.ts) or wrap the global WebSocket; the bundle is built by our esbuild with stubs (phone.ts STUBS) so no pinned file is patched. UNVERIFIED which hook; decide in migration step 6 with a spike.
- `{"event":"handshake","seq","stage":"hello|ready|auth|authenticated|error","dir","payload":<json>}`: hello/ready are plaintext JSON with keys/nonces replaced by shape placeholders (`<b64:32>`), auth/authenticated are decrypted sealed JSON, `e2ee_error` on rejection; ciphertext is never emitted. Interpreter shapes these as `channel:"handshake"` steps only when `capture_handshake` is true.
- `{"event":"close","seq","code":n,"reason":"...","wasClean":bool}`: from the WebSocket close event (DirectRpcClient exposes only states, not numeric codes). Codes orca-serve implements: 4001 auth/handshake, 4002 handshake deadline 10 s, 4003 too many undecryptable frames (clis/orca-serve/src/transport.rs:1-4).

Step mapping (Rust side): request -> c2s `{method,params}` rid + s2c response rid (today's shape, orca/client.ts:93-119); subscribe -> c2s opener, then every `frame` event with that rid as s2c steps until the interpreter's listen window ends, then `unsubscribe`; binary frames `kind:binary` with `len`; handshake events -> `channel:"handshake"` steps; close event -> `ws:close` step. Existing cassette rows stay byte-identical because a legacy script requests no taps and no handshake capture. Drain guarantee: before replying to `close`/`unsubscribe` the sidecar flushes all queued events, so no frame is lost between the window end and the reply.

Rust side port: `Phone` in section ports, `ScriptedPhone` double in core testing, `SidecarPhone` adapter in mimic-script-orca spawning `node tooling/mimic-drivers/src/orca-phone.ts`.
