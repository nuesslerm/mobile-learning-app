# The T3 chain does not run the real agent CLIs

`wf mimic scan` and the `wf-mimic` driver still resolve real CLIs (`T3_CHAIN_CLIS`, `MIMIC_REAL_CLI_DIR`), and the rca backends run them in `real-backends.yml`. The T3 integration chain (`clis/t3-serve/tests/integration`) runs the scripted CLIs only.

Why: running them from that chain linked `wf-mimic` (a versioned library, with `secrets` behind it) into `t3-serve`'s cargo closure. The image then had to copy `clis/mimic`, which `no_image_copies_the_source_of_a_versioned_library_except_the_one_that_builds_it` forbids, and every secrets change rebuilt the T3 image.

To bring it back without that link, resolve the credentials in a launcher outside the crate (a `wf mimic` verb that execs the test with the resolved environment) and read only the environment in the rig. It needs a credential per CLI, which the vault does not hold yet.
