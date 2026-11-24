Write ARCHITECTURE.md for this repository: the one short document that leads a bundle a chat assistant reads by retrieving chunks of it, so any single chunk must be able to find the global rules. Reply with the markdown document only, no preamble and no code fence around it.

Source material below: the repository's CLAUDE.md (the layering rules), CONCEPTS.md (vocabulary) and a repomix `--compress` skeleton (signatures only). Use only what they show; name a package, export or dependency you cannot see in them as unknown rather than guessing.

Required sections, in this order, dense and in plain bullets (target 1,500 to 3,000 words):

1. `# Architecture`: two sentences on what the repository is.
2. `## Layering rules`: the dependency direction per package type (ports and adapters: dependencies point inward to a pure core, the core owns its ports, adapters implement them and one composition root wires them; for frontend features `domain/`, `data/`, `state/`, `ui/` and the composition root, for Rust the `core`/`rs` crate pairs, and the `wf` crate split), what `domain/` may never do, and how a feature is imported (barrel, `routes/*`, `testing`).
3. `## Packages`: one bullet per top-level package or feature, as `path: role; main exports; depends on`. Cover every `frontend/mla-tamagui/features/*`, every `backend/*` service and every `clis/*` crate that appears in the skeleton.
4. `## Key flows`: the three to six flows that cross packages (a deploy, a login capture, a test run, an EAS update), each as a short arrow chain of paths.
5. `## Where to look`: a table-free list mapping common questions to the path to open first.

Every bullet cites repo-relative paths in backticks. Do not describe test internals, and do not repeat CLAUDE.md text that is not a layering or ownership fact.
