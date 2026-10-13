# Native macOS nested-state probe

This manual probe executes a tiny EL2 counter, captures the production HVF CPU
and GIC state, and attempts two fresh-VM restores of that counter. It does not
qualify a running L2 VM, nested interrupts, or full sandbox snapshots. The VMM
nested-snapshot admission guard remains closed.

Run on an Apple Silicon Mac that supports EL2, with the Hypervisor entitlement:

```sh
cargo build --manifest-path tests/nested-restore-hvf/Cargo.toml
codesign --force -s - --entitlements /path/to/msb-entitlements.plist \
  tests/nested-restore-hvf/target/debug/nested-hvf-probe
tests/nested-restore-hvf/target/debug/nested-hvf-probe
```

The guest run has a two-second watchdog. A capture failure exits nonzero after
destroying the test VM. No register is silently omitted to make capture succeed.

Observed on Apple M5 Pro, macOS 26.5.1 (25F80): EL2 execution increments the
counter, but `hv_vcpu_get_sys_reg(MDCR_EL2)` returns `0xfae94003`
(`HV_BAD_ARGUMENT`). Production CPU capture therefore fails before either
restore attempt. This is a snapshot blocker, not evidence that EL2 execution is
unsupported.

`macos-capability.c` separately queries the framework's EL2 capability. A true
result only advertises execution support; it does not establish snapshot support.
