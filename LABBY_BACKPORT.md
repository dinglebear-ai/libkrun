# Labby VMM security backport

This branch preserves the `msb_krun_vmm` 0.1.39 source from upstream commit
`2bd0f84ad0956f3032e0490d3b8512b6851eca12` and updates zstd to 0.14.3,
zstd-safe to 8.2.0, and zstd-sys to 2.1.2. These versions address
RUSTSEC-2026-0391, RUSTSEC-2026-0398, RUSTSEC-2026-0403, and
RUSTSEC-2026-0410.

The VMM manifest uses registry dependencies for its sibling crates, matching
the published 0.1.39 package. This prevents a Git patch from introducing
source-distinct sibling types alongside Labby's existing registry packages.
Labby's lockfile retains those sibling versions at 0.1.39.

This branch supports Labby's VMM dependency backport. It does not establish
compatibility for the entire libkrun workspace or native Make targets.
Upstream licensing and attribution remain intact.

Local Linux validation compiled the VMM with `blk,net,devices/net`, passed
41 hardware-independent existing tests, and passed two direct zstd tests
covering a 64 KiB decoding round trip and malformed/truncated input.
Labby's Code Mode consumer also passed its all-features check. No VM hardware
tests or macOS qualification were performed. The older upstream workspace
lockfile has separate pre-existing advisories; Labby's consumer lockfile
passed the advisory audit.

Consumers should pin an immutable revision. Replace this backport with a
compatible fixed upstream release when one is available.
