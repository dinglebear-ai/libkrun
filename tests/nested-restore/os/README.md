# Running Linux and disk restore workload

This fixture runs a Linux kernel under QEMU with mandatory KVM acceleration inside
libkrun. The inner PID 1 repeatedly commits an ext4 file, evicts its clean file
pages, reads the data back, and sleeps on a timer. The expected record and workload
identity stay in memory. A mismatch stops progress; a reboot against the existing
disk fails its exclusive file creation. The host harness also requires the same
identity and ten seconds of continued progress after restore.

Build on Linux matching the guest architecture, with a C compiler, static libc,
`busybox-static`, QEMU system binaries, `cpio`, `e2fsprogs`, and `sudo` available:

```sh
tests/nested-restore/os/build.sh /tmp/nested-os /path/to/inner-linux-kernel
```

The inner kernel must have initramfs, devtmpfs, ext4, virtio block and console
support built in. x86 uses Q35, PCI virtio block and `ttyS0`; ARM64 uses the `virt`
machine, MMIO virtio block and `ttyAMA0`. Supply a bzImage on x86 or an Image on
ARM64. The outer kernel needs KVM and the matching hardware virtualization backend.

Use the [shared harness](../README.md), with these additional settings:

```sh
export TEST_INITRAMFS=/tmp/nested-os/parent.cpio
export TEST_MEMORY_MIB=1024
export TEST_PROGRESS_TIMEOUT_SECONDS=120
```

The longer deadline allows Linux to boot; the continuous ten-second observation
window still resets whenever progress stalls. Only complete console records are
accepted, so a partially written identity cannot masquerade as a reboot.

On Linux x86, embed `parent.cpio` in the outer firmware kernel with
`CONFIG_INITRAMFS_SOURCE`; its mmap boot path does not consume the external
initramfs. macOS and Windows load `TEST_INITRAMFS` directly. Ordinary runtime builds
still reject nested capture: diagnostic qualification requires the isolated
admission bypass described in the shared harness documentation.

The disk image lives in the outer guest's RAM-backed root filesystem. This checks
inner filesystem consistency, block I/O, QEMU state, CPU state and timers across
whole-VM capture. It does not qualify host-backed persistent files, external
mounts, networking, cross-host migration or the Microsandbox lifecycle API.

2026-10-14 qualification: Intel Linux, AMD Windows and Apple M5 Pro macOS passed
capture, fresh-process restore, two concurrent forks and a second snapshot
generation. AMD Linux also passed Microsandbox's managed-disk CLI matrix,
including archive export/import, with the default inner guest settings and no
timer overrides. That runtime uses a split interrupt controller for the outer VM;
select `TEST_SPLIT_IRQCHIP=true` in this harness to match it. The standalone
fully in-kernel outer-controller profile still has early boot delays on the AMD
cloud host, so keep its results separate. Disabling HPET in the inner guest also
avoids that delay, but is not required by the tested Microsandbox configuration.
Linux ARM64, Intel Windows and Windows ARM64 still need native qualification.
The public nested-snapshot guard remains closed.
