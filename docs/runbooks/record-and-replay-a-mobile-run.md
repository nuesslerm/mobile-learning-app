# Record a mobile run and replay it with the server stopped

`wf mimic proxy record` keeps what a client and its server said as a cassette set, with every credential replaced by a fixture value. `wf mimic proxy replay` answers the unmodified client from that set, with no server. This runbook records the T3 Code iOS app opening a fixture chat against the pinned T3 server, stops the server and opens the chat again through the replay. To read the shape of the traffic instead, use [the shape trace](observe-the-native-t3-mobile-app.md).

Verified on 26 Nov 2026 with T3 Code 2.3.0 on an iPhone 18 Pro Max simulator (iOS 27.0) and T3 0.1.9: the [recorded set](../plans/orca-mobile-interface/reference/t3/native-open-chat/) is the result.

## 1. A simulator of its own

The reference simulator `rca` is only read. The new one takes its model, runtime and the named apps.

```sh
wf test prepare-ios --fresh mimic-proxy --apps-from rca \
  --app com.t3tools.t3code --app com.stably.orca.mobile --app host.exp.Exponent \
  --scheme t3code=com.t3tools.t3code
```

The command prints the simulator's `udid`. The device verbs below need `WF_IDB_PYTHON` (the Python that holds fb-idb) and `WF_IOS_APP_ID=com.t3tools.t3code`.

## 2. A reference with synthetic state

Start the T3 distribution of [the pin](../../tooling/t3-conformance/t3-pin.json) on loopback with its own state directory, an isolated `HOME`, a fixture Git repository and a `PATH` whose `claude`, `codex` and `opencode` answer a version query and nothing else. No provider is reached: a turn fails at once and the chat shows the user's message and the provider error. Create one project for the repository, one thread and one message over the orchestration socket (`projects.mutate`, then `orchestration.dispatchCommand` with `thread.create` and `message.dispatch`).

Use made-up text and a made-up repository only. The set is published.

## 3. Record

```sh
wf mimic proxy record --set <set-dir> --upstream http://127.0.0.1:18598 \
  --listen 127.0.0.1:18599 --preset t3 --mask "<reference-dir>=/fixture/reference" \
  --trace /tmp/t3-open-chat.shape.jsonl --duration 900
```

A `<set-dir>/mimic.toml` written before the run stamps the set as a recording of the pin:

```toml
rules = "rules.json"

[reference]
name = "t3"
driver = "wire"
pin = "<path to>/tooling/t3-conformance/t3-pin.json"
version_pointer = "/version"
commit_pointer = "/commit"

[proxy.replay]
ignore_keys = ["httpSnapshotFallback", "afterSequence"]
```

Pair the app with the **proxy address**: in T3 Code open Add environment, enter `127.0.0.1:18599` and a code from `t3 pair --base-dir <state>`. The simulator reaches the Mac's loopback. Then open the chat. Stop the recorder with SIGTERM or let the duration run out; it prints what it wrote and how many fixture values stand in for credentials.

The fresh simulator may expose no accessibility tree to `wf ios`. Taps by point work (`wf ios tap-on --point X,Y`), and `wf ios input-text` types into the focused field.

Check before publishing: `wf mimic verify <set-dir>` is `ok` with `kind: recorded`, and a search of `interactions.jsonl` for the host name, the user name and the workspace path finds nothing. The recorder masks the home and temp directory, the host and user name and tailnet names by itself; `--mask` adds the rest.

## 4. Replay with the server stopped

Stop the reference. Then:

```sh
WF_TEST_OUTPUT_DIR=/tmp/out wf mimic proxy replay <set-dir> --listen 127.0.0.1:18599 \
  --mask "<reference-dir>=/fixture/reference" --suite wf-mimic-proxy-t3-native --duration 900
wf ios open-link "t3code://" --relaunch --device <udid>
```

Open the chat again. The same messages appear and the header shows no reconnect. Stop the replay and read its output: `ok: true`, `upstream_connections: 0`, `mismatch_count: 0`. `--mask` is needed on the machine that recorded, because the app still holds real paths from the recorded run; on another machine the app only ever sees fixture values.

When the app asks for something the set does not have, the replay closes that socket with code 1011 (or answers 502), exits 1 and names the route or RPC method, the nearest recording (`interactions.jsonl:<line>`, step) and the fields that differ. Two kinds of difference are normal for an unmodified client and are settled in the set, not in the client: a key it sends only on some runs goes into `[proxy.replay] ignore_keys`, and an id or time it makes up is masked by a rule in `rules.json`.

## 5. A client that takes no server address: the explicit proxy

When the app cannot be pointed at a server address, record with `--allow-host` instead of `--upstream`, and tell the client to use the address the verb listens on as its HTTP and HTTPS proxy.

```sh
wf mimic proxy record --set <set-dir> --allow-host api.fixture.test --ca-out /tmp/mimic-ca.pem \
  --listen 127.0.0.1:18700 --duration 300
wf mimic proxy replay <set-dir> --allow-host api.fixture.test --ca-out /tmp/mimic-ca.pem --listen 127.0.0.1:18700
```

The test environment must trust `/tmp/mimic-ca.pem`, the certificate of a CA made for that run; its key is never written. Approved hosts are intercepted and their own certificates validated; every other host is tunnelled unread while recording and refused while replaying. Read `gaps` in the output before trusting a recording: `rejected_ca` is a client that pins or distrusts the CA, `never_seen` is an approved host the client reached some other way or not at all, `tunnelled` is traffic nobody read.

This mode is verified against a local HTTPS server with a test client, not yet on a device: nothing sets a simulator's or an emulator's proxy and trust store for you, and nothing restores them.

## What this covers

- Client: T3 Code 2.3.0 (iOS simulator). Journey: launch already paired, reconnect, open one chat.
- Protocols on the device: HTTP/1.1 and WebSocket to a server address the app is configured with.
- The explicit HTTPS proxy: record and replay of an HTTPS exchange against a local server (flow tests), with the gaps named. No device journey has run through it.
- `wf mimic proxy check`: the recorded set against a replay of itself, and a candidate with one changed reply. Not yet against `rca-serve`.
