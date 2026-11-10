//! The System widget's numbers: how busy the processor and the graphics
//! card are and how much memory is in use. Read off the interface thread;
//! each read measures since the one before.

/// The physical memory in use, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Memory {
    pub used: u64,
    pub total: u64,
}

impl Memory {
    pub fn percent(&self) -> f32 {
        if self.total == 0 {
            return 0.;
        }
        (self.used as f64 / self.total as f64 * 100.) as f32
    }

    /// "11/32 GB".
    pub fn label(&self) -> String {
        const GB: f64 = 1024. * 1024. * 1024.;
        format!(
            "{:.0}/{:.0} GB",
            self.used as f64 / GB,
            self.total as f64 / GB
        )
    }
}

/// What the system says now; a value it does not give is `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    /// All processors together, 0 to 100.
    pub cpu: Option<f32>,
    pub memory: Option<Memory>,
    /// The kernel's memory pressure where it has one (macOS): 1 normal,
    /// 2 tight, 4 critical.
    pub pressure: Option<u32>,
    /// The busiest engine of any graphics card, 0 to 100, as the Task
    /// Manager counts it.
    pub gpu: Option<f32>,
}

/// How many readings the history keeps: a minute at one every two seconds.
pub const HISTORY: usize = 30;

/// The last minute of each number, oldest first, for the widget's graphs.
/// A reading without a number leaves that number's graph as it was.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    pub cpu: std::collections::VecDeque<f32>,
    pub memory: std::collections::VecDeque<f32>,
    pub gpu: std::collections::VecDeque<f32>,
}

impl History {
    pub fn push(&mut self, stats: &Stats) {
        let keep = |graph: &mut std::collections::VecDeque<f32>, value: Option<f32>| {
            let Some(value) = value else {
                return;
            };
            graph.push_back(value.clamp(0., 100.));
            while graph.len() > HISTORY {
                graph.pop_front();
            }
        };
        keep(&mut self.cpu, stats.cpu);
        keep(
            &mut self.memory,
            stats.memory.map(|memory| memory.percent()),
        );
        keep(&mut self.gpu, stats.gpu);
    }
}

/// The processors' load between two readings of the system's clocks:
/// `idle`, and `busy` (kernel and user time, the idle time included, as
/// Windows reports them).
pub fn cpu_between(before: (u64, u64), now: (u64, u64)) -> Option<f32> {
    let idle = now.0.checked_sub(before.0)?;
    let all = now.1.checked_sub(before.1)?;
    if all == 0 {
        return None;
    }
    Some(((1. - idle as f64 / all as f64) * 100.).clamp(0., 100.) as f32)
}

/// The graphics load out of Windows' "GPU Engine" counters. Each instance is
/// one process on one engine (`pid_4_luid_0x0_0x1_phys_0_eng_3_engtype_3D`):
/// an engine's load is the sum over its processes, and the card is as busy
/// as its busiest engine.
pub fn gpu_load<'a>(instances: impl IntoIterator<Item = (&'a str, f64)>) -> f32 {
    let mut engines: std::collections::HashMap<&str, f64> = std::collections::HashMap::new();
    for (name, value) in instances {
        if !value.is_finite() || value <= 0. {
            continue;
        }
        let engine = name.find("luid_").map_or(name, |at| &name[at..]);
        *engines.entry(engine).or_default() += value;
    }
    engines.into_values().fold(0., f64::max).clamp(0., 100.) as f32
}

/// The system's numbers now. The first call waits a moment to have
/// something to measure against, so it never runs on the interface thread.
pub fn read() -> Stats {
    #[cfg(windows)]
    {
        win::read()
    }
    #[cfg(not(windows))]
    {
        let usage = crate::resources::usage();
        #[cfg(target_os = "macos")]
        let (memory, gpu) = (mac::memory(), mac::gpu());
        #[cfg(not(target_os = "macos"))]
        let (memory, gpu) = (None, None);
        Stats {
            cpu: usage.cpu_percent.map(|cpu| cpu as f32),
            memory,
            pressure: usage.memory_pressure,
            gpu,
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::Memory;
    use crate::notch::cf::{self, Owned};

    /// `vm_statistics64` as the kernel fills it.
    #[repr(C, align(8))]
    #[derive(Default)]
    struct VmStatistics {
        free: u32,
        active: u32,
        inactive: u32,
        wired: u32,
        zero_fill: u64,
        reactivations: u64,
        pageins: u64,
        pageouts: u64,
        faults: u64,
        cow_faults: u64,
        lookups: u64,
        hits: u64,
        purges: u64,
        purgeable: u32,
        speculative: u32,
        decompressions: u64,
        compressions: u64,
        swapins: u64,
        swapouts: u64,
        compressor: u32,
        throttled: u32,
        external: u32,
        internal: u32,
        uncompressed_in_compressor: u64,
    }

    /// HOST_VM_INFO64.
    const VM_INFO: i32 = 4;

    unsafe extern "C" {
        fn mach_host_self() -> u32;
        fn host_statistics64(host: u32, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
        fn host_page_size(host: u32, size: *mut usize) -> i32;
    }

    /// The memory in use as Activity Monitor counts it: the apps' memory,
    /// the wired memory and what the compressor holds.
    pub(super) fn memory() -> Option<Memory> {
        let mut total = 0u64;
        let mut length = std::mem::size_of_val(&total);
        // SAFETY: plain kernel reads into values sized for them.
        unsafe {
            if libc::sysctlbyname(
                c"hw.memsize".as_ptr(),
                (&raw mut total).cast(),
                &mut length,
                std::ptr::null_mut(),
                0,
            ) != 0
                || total == 0
            {
                return None;
            }
            let host = mach_host_self();
            let mut page = 0usize;
            if host_page_size(host, &mut page) != 0 || page == 0 {
                return None;
            }
            let mut vm = VmStatistics::default();
            let mut count = (std::mem::size_of::<VmStatistics>() / 4) as u32;
            if host_statistics64(host, VM_INFO, (&raw mut vm).cast(), &mut count) != 0 {
                return None;
            }
            let pages = u64::from(vm.internal.saturating_sub(vm.purgeable))
                + u64::from(vm.wired)
                + u64::from(vm.compressor);
            Some(Memory {
                used: (pages * page as u64).min(total),
                total,
            })
        }
    }

    /// The graphics card's load, from its accelerator's own statistics
    /// (`Device Utilization %` on Apple silicon, `GPU Activity(%)` on some
    /// others); the busiest card where there are several.
    pub(super) fn gpu() -> Option<f32> {
        #[link(name = "IOKit", kind = "framework")]
        unsafe extern "C" {
            fn IOServiceMatching(name: *const i8) -> cf::Ref;
            fn IOServiceGetMatchingServices(main: u32, matching: cf::Ref, found: *mut u32) -> i32;
            fn IOIteratorNext(iterator: u32) -> u32;
            fn IOObjectRelease(object: u32) -> i32;
            fn IORegistryEntryCreateCFProperty(
                entry: u32,
                key: cf::Ref,
                allocator: cf::Ref,
                options: u32,
            ) -> cf::Ref;
        }
        let key = cf::string(c"PerformanceStatistics")?;
        // SAFETY: the matching dictionary is consumed by the lookup; every
        // entry and the iterator are released, and each property is owned.
        unsafe {
            let matching = IOServiceMatching(c"IOAccelerator".as_ptr());
            if matching.is_null() {
                return None;
            }
            let mut iterator = 0u32;
            if IOServiceGetMatchingServices(0, matching, &mut iterator) != 0 {
                return None;
            }
            let mut busiest: Option<f64> = None;
            loop {
                let entry = IOIteratorNext(iterator);
                if entry == 0 {
                    break;
                }
                if let Some(statistics) = Owned::new(IORegistryEntryCreateCFProperty(
                    entry,
                    key.0,
                    std::ptr::null(),
                    0,
                )) {
                    let load = cf::number(statistics.0, c"Device Utilization %")
                        .or_else(|| cf::number(statistics.0, c"GPU Activity(%)"));
                    if let Some(load) = load {
                        busiest = Some(busiest.map_or(load, |most| most.max(load)));
                    }
                }
                IOObjectRelease(entry);
            }
            IOObjectRelease(iterator);
            busiest.map(|load| load.clamp(0., 100.) as f32)
        }
    }
}

#[cfg(windows)]
mod win {
    use super::{cpu_between, gpu_load, Memory, Stats};
    use std::sync::Mutex;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
        PDH_MORE_DATA,
    };
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetSystemTimes;

    /// The graphics counters, kept open between reads: they are rates, so
    /// each read needs the one before.
    struct Gpu {
        query: PDH_HQUERY,
        counter: PDH_HCOUNTER,
    }
    // The handles are only used under the sampler's lock.
    unsafe impl Send for Gpu {}

    impl Gpu {
        fn open() -> Option<Self> {
            let path: Vec<u16> = "\\GPU Engine(*)\\Utilization Percentage\0"
                .encode_utf16()
                .collect();
            unsafe {
                let mut query: PDH_HQUERY = std::ptr::null_mut();
                if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != 0 {
                    return None;
                }
                let mut counter: PDH_HCOUNTER = std::ptr::null_mut();
                if PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) != 0 {
                    PdhCloseQuery(query);
                    return None;
                }
                PdhCollectQueryData(query);
                Some(Self { query, counter })
            }
        }

        fn read(&self) -> Option<f32> {
            unsafe {
                if PdhCollectQueryData(self.query) != 0 {
                    return None;
                }
                let (mut size, mut count) = (0u32, 0u32);
                if PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut size,
                    &mut count,
                    std::ptr::null_mut(),
                ) != PDH_MORE_DATA
                {
                    return None;
                }
                // Items, then the names they point to; u64s keep it aligned.
                let mut buffer = vec![0u64; size as usize / 8 + 1];
                let items = buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
                if PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut size,
                    &mut count,
                    items,
                ) != 0
                {
                    return None;
                }
                let items = std::slice::from_raw_parts(items, count as usize);
                let named: Vec<(String, f64)> = items
                    .iter()
                    .filter(|item| item.FmtValue.CStatus == 0 && !item.szName.is_null())
                    .map(|item| {
                        let mut length = 0;
                        while *item.szName.add(length) != 0 {
                            length += 1;
                        }
                        let name = std::slice::from_raw_parts(item.szName, length);
                        (
                            String::from_utf16_lossy(name),
                            item.FmtValue.Anonymous.doubleValue,
                        )
                    })
                    .collect();
                Some(gpu_load(
                    named.iter().map(|(name, value)| (name.as_str(), *value)),
                ))
            }
        }
    }

    impl Drop for Gpu {
        fn drop(&mut self) {
            unsafe {
                PdhCloseQuery(self.query);
            }
        }
    }

    #[derive(Default)]
    struct Sampler {
        /// Idle, and kernel plus user, at the last read.
        times: Option<(u64, u64)>,
        /// The counters once opened; `Some(None)` where there are none.
        gpu: Option<Option<Gpu>>,
    }

    static SAMPLER: Mutex<Sampler> = Mutex::new(Sampler {
        times: None,
        gpu: None,
    });

    fn times() -> Option<(u64, u64)> {
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut idle, mut kernel, mut user) = (zero, zero, zero);
        if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
            return None;
        }
        let ticks =
            |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
        Some((ticks(idle), ticks(kernel) + ticks(user)))
    }

    fn memory() -> Option<Memory> {
        let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
            return None;
        }
        Some(Memory {
            used: status.ullTotalPhys.saturating_sub(status.ullAvailPhys),
            total: status.ullTotalPhys,
        })
    }

    pub(super) fn read() -> Stats {
        let mut sampler = SAMPLER.lock().unwrap_or_else(|error| error.into_inner());
        if sampler.times.is_none() || sampler.gpu.is_none() {
            // Nothing to measure against yet: take a first reading and wait.
            sampler.times = times();
            sampler.gpu = Some(Gpu::open());
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        let now = times();
        let cpu = match (sampler.times, now) {
            (Some(before), Some(now)) => cpu_between(before, now),
            _ => None,
        };
        sampler.times = now;
        Stats {
            cpu,
            memory: memory(),
            pressure: None,
            gpu: sampler
                .gpu
                .as_ref()
                .and_then(Option::as_ref)
                .and_then(Gpu::read),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_processor_load_is_the_time_not_idle() {
        // 1000 ticks passed on all processors, 750 of them idle.
        assert_eq!(cpu_between((1000, 4000), (1750, 5000)), Some(25.));
        // No time passed, or a clock that went back: nothing to say.
        assert_eq!(cpu_between((1000, 4000), (1000, 4000)), None);
        assert_eq!(cpu_between((1000, 4000), (900, 5000)), None);
    }

    #[test]
    fn the_graphics_load_is_the_busiest_engine_over_its_processes() {
        let load = gpu_load([
            ("pid_10_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 20.),
            ("pid_22_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 15.5),
            ("pid_10_luid_0x0_0x1_phys_0_eng_5_engtype_VideoDecode", 30.),
            ("pid_10_luid_0x0_0x2_phys_0_eng_0_engtype_3D", 1.),
            ("pid_77_luid_0x0_0x2_phys_0_eng_0_engtype_3D", f64::NAN),
        ]);
        assert_eq!(load, 35.5);
        assert_eq!(gpu_load([]), 0.);
        // Many processes on one engine never read past the whole of it.
        assert_eq!(
            gpu_load([
                ("pid_1_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 70.),
                ("pid_2_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 60.),
            ]),
            100.
        );
    }

    #[test]
    fn memory_reads_as_a_share_and_in_gigabytes() {
        let gb = 1024 * 1024 * 1024;
        let memory = Memory {
            used: 8 * gb,
            total: 32 * gb,
        };
        assert_eq!(memory.percent(), 25.);
        assert_eq!(memory.label(), "8/32 GB");
        assert_eq!(Memory { used: 1, total: 0 }.percent(), 0.);
    }

    #[test]
    fn the_history_keeps_the_last_minute_of_each_number() {
        let mut history = History::default();
        for step in 0..HISTORY + 5 {
            history.push(&Stats {
                cpu: Some(step as f32),
                // The graphics load is not always given.
                gpu: (step % 2 == 0).then_some(250.),
                ..Stats::default()
            });
        }
        assert_eq!(history.cpu.len(), HISTORY);
        assert_eq!(history.cpu.front(), Some(&5.));
        assert_eq!(history.cpu.back(), Some(&(HISTORY as f32 + 4.)));
        assert!(history.memory.is_empty());
        assert_eq!(history.gpu.len(), 18);
        assert_eq!(history.gpu.back(), Some(&100.));
    }

    /// The real counters of the machine the tests run on.
    #[cfg(windows)]
    #[test]
    fn windows_gives_the_processor_and_the_memory() {
        let stats = read();
        let cpu = stats.cpu.expect("processor load");
        assert!((0. ..=100.).contains(&cpu));
        let memory = stats.memory.expect("memory");
        assert!(memory.total > 0 && memory.used <= memory.total);
        if let Some(gpu) = stats.gpu {
            assert!((0. ..=100.).contains(&gpu));
        }
        eprintln!("stats: {stats:?}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn this_mac_gives_the_memory_and_the_graphics_load() {
        let stats = read();
        let memory = stats.memory.expect("memory");
        assert!(memory.total > 0 && memory.used > 0 && memory.used <= memory.total);
        if let Some(gpu) = stats.gpu {
            assert!((0. ..=100.).contains(&gpu));
        }
        eprintln!("stats: {stats:?} memory {}", memory.label());
    }
}
