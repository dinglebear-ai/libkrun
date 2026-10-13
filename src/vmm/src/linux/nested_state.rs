//! Bounded Linux x86 nested-state transport. The VMM admission gate remains closed
//! until fresh-VM restore is qualified on both Intel and AMD hosts.

use std::io;
use std::os::fd::AsRawFd;

use kvm_bindings::KVM_CAP_NESTED_STATE;
use kvm_ioctls::{Kvm, VcpuFd};

const HEADER_SIZE: usize = 128;
const MAX_STATE_SIZE: usize = 1024 * 1024;
// Linux x86 UAPI: _IOWR/_IOW(KVMIO, 0xbe/0xbf, struct kvm_nested_state).
const GET_NESTED_STATE: libc::c_ulong = 0xc080_aebe;
const SET_NESTED_STATE: libc::c_ulong = 0x4080_aebf;

fn capacity() -> io::Result<usize> {
    let capacity = Kvm::new()
        .map_err(io::Error::from)?
        .check_extension_raw(KVM_CAP_NESTED_STATE.into());
    if capacity < HEADER_SIZE as i32 || capacity as usize > MAX_STATE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "KVM nested-state export is unavailable or exceeds the supported bound",
        ));
    }
    Ok(capacity as usize)
}

fn validate(bytes: &[u8]) -> io::Result<()> {
    if !(HEADER_SIZE..=MAX_STATE_SIZE).contains(&bytes.len()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid KVM nested-state length",
        ));
    }
    let size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let format = u16::from_le_bytes(bytes[2..4].try_into().unwrap());
    if size != bytes.len() || format > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid KVM nested-state header",
        ));
    }
    Ok(())
}

pub(super) fn capture(vcpu: &VcpuFd) -> io::Result<Vec<u8>> {
    let capacity = capacity()?;
    // u64 storage provides alignment for the UAPI header; zero initialization also
    // prevents uninitialized padding from entering the snapshot.
    let mut words = vec![0_u64; capacity.div_ceil(8)];
    let bytes =
        unsafe { std::slice::from_raw_parts_mut(words.as_mut_ptr().cast::<u8>(), capacity) };
    bytes[4..8].copy_from_slice(&(capacity as u32).to_le_bytes());
    // SAFETY: storage is aligned, writable, and covers the advertised capacity.
    if unsafe { libc::ioctl(vcpu.as_raw_fd(), GET_NESTED_STATE, bytes.as_mut_ptr()) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let used = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    if used > capacity {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "KVM returned oversized nested state",
        ));
    }
    let state = &bytes[..used];
    validate(state)?;
    Ok(state.to_vec())
}

pub(super) fn validate_destination(bytes: &[u8]) -> io::Result<()> {
    validate(bytes)?;
    if bytes.len() > capacity()? {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "destination KVM cannot accept this nested-state size",
        ));
    }
    Ok(())
}

pub(super) fn restore(vcpu: &VcpuFd, bytes: &[u8]) -> io::Result<()> {
    validate_destination(bytes)?;
    let mut words = vec![0_u64; bytes.len().div_ceil(8)];
    // SAFETY: the allocation covers bytes.len() bytes and is suitably aligned.
    let aligned =
        unsafe { std::slice::from_raw_parts_mut(words.as_mut_ptr().cast::<u8>(), bytes.len()) };
    aligned.copy_from_slice(bytes);
    // KVM validates VMX/SVM format, flags and guest physical addresses against
    // the configured destination vCPU. Never substitute an empty state on error.
    if unsafe { libc::ioctl(vcpu.as_raw_fd(), SET_NESTED_STATE, aligned.as_ptr()) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires Linux x86 KVM with nested-state support"]
    fn nested_state_transfers_between_fresh_vcpus() {
        let kvm = Kvm::new().unwrap();
        let cpuid = kvm
            .get_supported_cpuid(kvm_bindings::KVM_MAX_CPUID_ENTRIES)
            .unwrap();
        let source_vm = kvm.create_vm().unwrap();
        let source = source_vm.create_vcpu(0).unwrap();
        source.set_cpuid2(&cpuid).unwrap();
        let saved = capture(&source).unwrap();
        let destination_vm = kvm.create_vm().unwrap();
        let destination = destination_vm.create_vcpu(0).unwrap();
        destination.set_cpuid2(&cpuid).unwrap();
        restore(&destination, &saved).unwrap();
        assert_eq!(capture(&destination).unwrap(), saved);
    }

    #[test]
    fn nested_state_rejects_invalid_wire_headers() {
        let mut state = vec![0; HEADER_SIZE];
        state[4..8].copy_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
        for format in [0_u16, 1] {
            state[2..4].copy_from_slice(&format.to_le_bytes());
            validate(&state).unwrap();
        }
        assert!(validate(&state[..HEADER_SIZE - 1]).is_err());
        state[2..4].copy_from_slice(&2_u16.to_le_bytes());
        assert!(validate(&state).is_err());
        state[2..4].copy_from_slice(&0_u16.to_le_bytes());
        state[4..8].copy_from_slice(&((HEADER_SIZE + 1) as u32).to_le_bytes());
        assert!(validate(&state).is_err());
    }
}
