//! The process's threads and the CPU time each has had (for the
//! performance overlay's busiest threads).

use std::time::Duration;

pub struct Thread {
    /// The system's id for it (stable while it lives).
    pub id: u64,
    pub name: String,
    /// User and system time so far.
    pub cpu: Duration,
}

/// The process's threads now (empty where the platform does not tell).
pub fn threads() -> Vec<Thread> {
    let mut threads = Vec::new();
    // SAFETY: the thread list is the kernel's, read and released here.
    #[cfg(target_vendor = "apple")]
    unsafe {
        unsafe extern "C" {
            static mach_task_self_: libc::mach_port_t;
            fn mach_port_deallocate(task: libc::mach_port_t, name: libc::mach_port_t) -> libc::kern_return_t;
        }
        let task = mach_task_self_;
        let mut list: libc::thread_act_array_t = std::ptr::null_mut();
        let mut count = 0;
        if libc::task_threads(task, &mut list, &mut count) != libc::KERN_SUCCESS {
            return threads;
        }
        for i in 0..count as usize {
            let port = *list.add(i);
            let mut info: libc::thread_extended_info = std::mem::zeroed();
            let mut n = libc::THREAD_EXTENDED_INFO_COUNT;
            let flavor = libc::THREAD_EXTENDED_INFO as libc::thread_flavor_t;
            if libc::thread_info(port, flavor, &mut info as *mut _ as libc::thread_info_t, &mut n) == libc::KERN_SUCCESS
            {
                let name = std::ffi::CStr::from_ptr(info.pth_name.as_ptr()).to_string_lossy().into_owned();
                let cpu = Duration::from_nanos(info.pth_user_time + info.pth_system_time);
                threads.push(Thread { id: port as u64, name, cpu });
            }
            mach_port_deallocate(task, port);
        }
        let bytes = count as usize * std::mem::size_of::<libc::thread_act_t>();
        libc::vm_deallocate(task, list as libc::vm_address_t, bytes as libc::vm_size_t);
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // SAFETY: sysconf only reads.
        let tick = 1e9 / unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64;
        let Ok(dir) = std::fs::read_dir("/proc/self/task") else { return threads };
        for entry in dir.flatten() {
            let Some(id) = entry.file_name().to_str().and_then(|s| s.parse().ok()) else { continue };
            let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else { continue };
            // "tid (name) state ...": the name may hold spaces and parentheses.
            let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else { continue };
            let fields: Vec<&str> = stat[close + 1..].split_whitespace().collect();
            // utime and stime, the 14th and 15th fields (the state is the 3rd).
            let ticks = |i: usize| fields.get(i).and_then(|f| f.parse::<u64>().ok()).unwrap_or(0);
            let cpu = Duration::from_nanos(((ticks(11) + ticks(12)) as f64 * tick) as u64);
            threads.push(Thread { id, name: stat[open + 1..close].to_owned(), cpu });
        }
    }
    // SAFETY: handles opened here are closed here; the description is freed.
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE, LocalFree};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        };
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcessId, GetThreadDescription, GetThreadTimes, OpenThread, THREAD_QUERY_LIMITED_INFORMATION,
        };
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return threads;
        }
        let process = GetCurrentProcessId();
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut more = Thread32First(snapshot, &mut entry) != 0;
        while more {
            if entry.th32OwnerProcessID == process {
                let thread = OpenThread(THREAD_QUERY_LIMITED_INFORMATION, 0, entry.th32ThreadID);
                if !thread.is_null() {
                    let mut t: [FILETIME; 4] = std::mem::zeroed();
                    let [created, exited, kernel, user] = &mut t;
                    if GetThreadTimes(thread, created, exited, kernel, user) != 0 {
                        // 100 ns units.
                        let units = |f: &FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
                        let mut name = String::new();
                        let mut description = std::ptr::null_mut();
                        if GetThreadDescription(thread, &mut description) >= 0 && !description.is_null() {
                            let len = (0..).take_while(|&i| *description.add(i) != 0).count();
                            name = String::from_utf16_lossy(std::slice::from_raw_parts(description, len));
                            LocalFree(description as _);
                        }
                        let cpu = Duration::from_nanos((units(kernel) + units(user)) * 100);
                        threads.push(Thread { id: entry.th32ThreadID as u64, name, cpu });
                    }
                    CloseHandle(thread);
                }
            }
            more = Thread32Next(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    threads
}

#[test]
fn this_thread_is_listed() {
    std::thread::Builder::new()
        .name("cpu test".into())
        .spawn(|| {
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_millis(30) {
                std::hint::black_box(0);
            }
            let threads = threads();
            if cfg!(any(target_vendor = "apple", target_os = "linux", target_os = "android", windows)) {
                let me = threads.iter().find(|t| t.name == "cpu test").expect("listed by name");
                assert!(me.cpu > Duration::ZERO);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
