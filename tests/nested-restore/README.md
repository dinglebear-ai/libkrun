# Live nested VM restore probe

This manual harness boots a Linux parent VM whose KVM child continuously updates
a counter and services timer interrupts. Capture and restore run in separate host
processes. Restore requires the original workload identity and continued timer
and counter progress, so rebooting the workload cannot pass.

The public nested-snapshot guard is still closed. An ordinary build must reject
capture. Backend qualification uses an isolated diagnostic build with only that
guard temporarily removed; never publish that build as a supported runtime.

Build `guest.c` as a static Linux x86 executable and place it at `/init` in an
initramfs containing `/dev/console` (character device 5:1) and `/dev`. The parent
kernel needs KVM, VMX/SVM, IRQ chip and PIT support. On Linux x86 the existing
firmware mmap boot path does not load an external initramfs, so embed it in the
test kernel with `CONFIG_INITRAMFS_SOURCE`. Windows loads `TEST_INITRAMFS` through
the kernel-copy path.

```sh
cargo build --manifest-path tests/nested-restore/Cargo.toml
export KRUNFW_PATH=/absolute/path/to/libkrunfw.so
export TEST_INITRAMFS=/absolute/path/to/initramfs.cpio
export TEST_ROOT=/absolute/path/to/empty-output-directory
export TEST_SNAPSHOT=/absolute/path/to/snapshot-directory
tests/nested-restore/target/debug/nested-restore-live capture
tests/nested-restore/target/debug/nested-restore-live restore
```

To capture again after restoring, set `TEST_OUTPUT_SNAPSHOT` to a new directory.
Restore that directory in another process to check a second generation. To test
forks, restore the same baseline into different `TEST_ROOT` directories. Do not
share output directories between concurrent runs. Require the printed `PASS`
marker after at least ten seconds of execution as well as exit code zero.
A guest shutdown before the controller finishes is a failure.

The fixture has no persistent guest disk or host filesystem mount. CPU, guest
memory and supported virtio device states are captured through production APIs.
It does not qualify sandbox filesystem recovery or CPU migration across hosts.

2026-10-13: Intel Linux/KVM passed capture, fresh-process restore, two independent
restores from one baseline, and capture/restore of a second generation using the
candidate KVM capability contract and the diagnostic guard bypass. AMD Linux and
Windows require their own live-restore results before admission can open.

On Windows the harness uses an explicit `hvc0` output device. The 2026-10-13 AMD
run captured successfully in a diagnostic build, but restore resumed the counter
without continued PIT interrupts. Progress lines include IF flags and PIC state
to help distinguish interrupt generation from a masked or in-service interrupt.
This failure must remain a failure; counter progress alone is insufficient.

## ARM64 on macOS

Build the KVM parent and its bare-metal ARM64 child together:

```sh
zig cc -target aarch64-linux-musl -static -pthread -Wall -Wextra -Werror \
  tests/nested-restore/guest-arm64.c tests/nested-restore/inner-arm64.S -o guest
```

Package `guest` as `/init` in the initramfs described above. The parent kernel
must include ARM KVM and VGICv3. The child runs at EL1, continuously updates RAM,
and handles virtual timer PPI 27 through its virtual GIC. Progress includes a
workload identity, interrupt count, fatal-exception flag, and parent monotonic time.

Use the same host harness and environment variables, with `KRUNFW_PATH` pointing
to an ARM64 libkrunfw dylib. macOS loads `TEST_INITRAMFS` through the kernel-copy
path. Codesign the exact host executable before running it:

```sh
codesign --force -s - --entitlements /path/to/msb-entitlements.plist \
  tests/nested-restore/target/debug/nested-restore-live
```

The ordinary build still rejects nested capture. Use a diagnostic build only
for backend qualification. In addition to the initial restore, recapture a
restored VM and restore that second snapshot: the original HVF ordering could
pass the first restore but stall on the second generation. The clock offset must
be installed before the saved timer registers so deadlines use the restored clock.

On 2026-10-13, Apple M5 Pro with macOS 26.5.1 passed diagnostic capture,
two concurrent restores from one baseline and a separate chain of four successive
restores. The same saved second-generation snapshot that stalled with the old
clock ordering resumed after the ordering fix. Another recaptured snapshot
reproducibly returned `KVM_EXIT_INTERNAL_ERROR`; this remains unresolved, so
these passes do not qualify public nested snapshots. The guest prints the KVM
internal-error data and kernel log on failure to support further investigation.
