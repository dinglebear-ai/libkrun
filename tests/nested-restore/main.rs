//! Each capture or restore runs in a fresh process. No public admission bypass.
use msb_krun::{
    GuestMemoryRange, MemoryCaptureOptions, MemoryCaptureSink, VmBuilder, VmMemoryRestoreSource,
    VmMemoryRestoreTarget,
};
use std::{
    fs,
    io::{self, Read, Write},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
struct Memory(fs::File);
impl MemoryCaptureSink for Memory {
    fn write_bytes(&mut self, range: GuestMemoryRange, bytes: &[u8]) -> io::Result<()> {
        self.0.write_all(&range.start().to_le_bytes())?;
        self.0.write_all(&range.length().to_le_bytes())?;
        self.0.write_all(bytes)
    }
    fn write_zero(&mut self, range: GuestMemoryRange) -> io::Result<()> {
        self.write_bytes(range, &vec![0; range.length() as usize])
    }
}
impl VmMemoryRestoreSource for Memory {
    fn restore(&mut self, target: &mut dyn VmMemoryRestoreTarget) -> io::Result<()> {
        loop {
            let mut start = [0; 8];
            if self.0.read(&mut start[..1])? == 0 {
                return Ok(());
            }
            self.0.read_exact(&mut start[1..])?;
            let mut size = [0; 8];
            self.0.read_exact(&mut size)?;
            let start = u64::from_le_bytes(start);
            let size = u64::from_le_bytes(size);
            if size > 512 * 1024 * 1024 {
                return Err(io::Error::other("oversized memory record"));
            }
            let mut bytes = vec![0; size as usize];
            self.0.read_exact(&mut bytes)?;
            target.write_bytes(
                GuestMemoryRange::new(start, size).map_err(io::Error::other)?,
                &bytes,
            )?;
        }
    }
}
fn progress(path: &std::path::Path) -> Option<(u32, u32, u64)> {
    let text = fs::read_to_string(path).ok()?;
    // Console writes can split a decimal identity at any digit. Ignore the
    // unfinished record so a partial number cannot look like a cold reboot.
    let (text, _) = text.rsplit_once('\n')?;
    text.lines().rev().find_map(|line| {
        let mut fields = line.strip_prefix("NESTED_PROGRESS ")?.split_whitespace();
        Some((
            fields.next()?.parse().ok()?,
            fields.next()?.parse().ok()?,
            fields.next()?.parse().ok()?,
        ))
    })
}
fn main() -> Result<()> {
    let mode = std::env::args()
        .nth(1)
        .ok_or("capture or restore required")?;
    let root = PathBuf::from(std::env::var("TEST_ROOT")?);
    let snapshot = PathBuf::from(std::env::var("TEST_SNAPSHOT")?);
    let saved = if mode == "restore" {
        Some(progress(&snapshot.join("progress")).ok_or("missing saved progress")?)
    } else {
        None
    };
    let memory_mib = std::env::var("TEST_MEMORY_MIB")
        .ok()
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(256);
    let progress_timeout = Duration::from_secs(
        std::env::var("TEST_PROGRESS_TIMEOUT_SECONDS")
            .ok()
            .map(|value| value.parse::<u64>())
            .transpose()?
            .unwrap_or(30),
    );
    let split_irqchip = std::env::var("TEST_SPLIT_IRQCHIP")
        .ok()
        .map(|value| value.parse::<bool>())
        .transpose()?
        .unwrap_or(false);
    if split_irqchip && !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err("TEST_SPLIT_IRQCHIP requires a Linux x86_64 host".into());
    }
    eprintln!("outer split irqchip: {split_irqchip}");
    fs::write(root.join("console.log"), "")?;
    let mut vm = VmBuilder::new()
        .on_exit(|code| {
            eprintln!("FAIL: guest exited with code {code} before the probe completed");
            std::process::exit(1);
        })
        .machine(|m| {
            m.vcpus(2)
                .memory_mib(memory_mib)
                .nested_virt(true)
                .split_irqchip(split_irqchip)
                .balloon(false)
                .rng(false)
        })
        .kernel(|k| {
            k.krunfw_path(std::env::var("KRUNFW_PATH").unwrap())
                .initramfs_path(std::env::var("TEST_INITRAMFS").unwrap())
        })
        .console(|c| {
            #[cfg(target_os = "windows")]
            {
                c.disable_implicit().virtio_output(root.join("console.log"))
            }
            #[cfg(not(target_os = "windows"))]
            {
                c.output(root.join("console.log"))
            }
        })
        .build()?;
    if mode == "restore" {
        vm.set_execution_restore(vmm::execution_state::ExecutionState::decode(&fs::read(
            snapshot.join("execution"),
        )?)?);
        vm.set_memory_restore(Memory(fs::File::open(snapshot.join("memory"))?));
        for entry in fs::read_dir(&snapshot)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with("device-") {
                vm.add_virtio_device_restore(vmm::device_state::VirtioDeviceState::decode(
                    &fs::read(entry.path())?,
                )?);
            }
        }
    } else if mode != "capture" {
        return Err("invalid mode".into());
    }
    let output_snapshot = std::env::var_os("TEST_OUTPUT_SNAPSHOT").map(PathBuf::from);
    let control = vm.control_handle();
    thread::spawn(move || {
        let result = (|| -> Result<()> {
            control.wait_until_running(Duration::from_secs(30))?;
            let begin = Instant::now();
            let mut previous = None;
            let mut progress_started = None;
            loop {
                if begin.elapsed() > progress_timeout {
                    return Err("inner counter/timer did not progress".into());
                }
                if let Some(current) = progress(&root.join("console.log")) {
                    if let Some(saved) = saved {
                        if current.2 != saved.2 {
                            return Err("inner workload cold-booted instead of restoring".into());
                        }
                        if current.1 <= saved.1 {
                            thread::sleep(Duration::from_millis(200));
                            continue;
                        }
                    }
                    if let Some(old) = previous {
                        let (old_count, old_ticks, _): (u32, u32, u64) = old;
                        if current.0 != old_count && current.1 > old_ticks && current.1 > 2 {
                            let started = progress_started.get_or_insert_with(Instant::now);
                            if started.elapsed() >= Duration::from_secs(10) {
                                println!("{mode}: inner counter={} timer={} (previous={old_count},{old_ticks})",current.0,current.1);
                                break;
                            }
                        } else {
                            progress_started = None;
                        }
                    }
                    previous = Some(current);
                } else {
                    progress_started = None;
                }
                thread::sleep(Duration::from_millis(200));
            }
            let _pause = control.pause()?;
            if mode == "capture" || output_snapshot.is_some() {
                let snapshot = output_snapshot.as_ref().unwrap_or(&snapshot);
                fs::create_dir_all(snapshot)?;
                fs::write(
                    snapshot.join("execution"),
                    control.capture_execution_state()?.encode()?,
                )?;
                let plan = control.plan_full_memory_capture()?;
                control.capture_memory(
                    &plan,
                    MemoryCaptureOptions::new(4096, false)?,
                    &mut Memory(fs::File::create(snapshot.join("memory"))?),
                )?;
                for (kind, id) in control.virtio_device_inventory()? {
                    let state = control.capture_virtio_device_state(kind, &id)?;
                    fs::write(
                        snapshot.join(format!("device-{kind}-{id}")),
                        state.encode()?,
                    )?;
                }
                fs::write(
                    snapshot.join("progress"),
                    fs::read(root.join("console.log"))?,
                )?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("FAIL: {error}");
            std::process::exit(1);
        }
        println!("PASS");
        std::process::exit(0);
    });
    vm.enter()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_waits_for_a_complete_console_record() {
        let path = std::env::temp_dir().join(format!(
            "nested-progress-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::write(
            &path,
            "NESTED_PROGRESS 1 10 123456\nNESTED_PROGRESS 2 20 123",
        )
        .unwrap();
        let partial = progress(&path);
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"456\n")
            .unwrap();
        let complete = progress(&path);
        fs::remove_file(path).unwrap();

        assert_eq!(partial, Some((1, 10, 123456)));
        assert_eq!(complete, Some((2, 20, 123456)));
    }
}
