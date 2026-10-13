# Live nested VM restore probe

This manual harness boots a Linux parent VM whose KVM child continuously updates
a counter and services PIT interrupts. Capture and restore run in separate host
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
marker as well as exit code zero: guest shutdown can terminate the runtime before
the controller finishes.

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
