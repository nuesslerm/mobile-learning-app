# Orca browser stream on macOS

The installed Orca desktop 1.4.255 served an isolated profile and a disposable repository. The Mimic phone sidecar used the reference DirectRpcClient at 1.4.259 (980c00247). Chromium opened a loopback fixture page. The iOS app was outside this run.

Mimic recorded three WebSocket connections to the reference endpoint. The first connection probed the first frame. The second created a page, subscribed with acknowledgements, received a JPEG, sent text and resubscribed at a wider viewport. The third reconnected with the same pairing, received another frame, closed the tab and checked its absence. The proxy recorded no transport failures. Its local cassette has authored provenance because the installed desktop executable was selected locally. [Transport shapes](transport.jsonl)

The sidecar observed the application events inside those encrypted connections. The selected request and response fields use aliases for generated IDs. Each frame entry includes the received byte length, dimensions and SHA-256 of the decoded JPEG. A successful insert acknowledgement covers the protocol response; a rendered input assertion belongs to the device test. [Decoded events](paired-client.json)
