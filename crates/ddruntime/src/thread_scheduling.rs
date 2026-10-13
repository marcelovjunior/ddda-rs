//! Forward guest scheduling requests, without choosing new priorities or
//! reordering jobs. DDDA 2017-12-13 Steam: 00DBDF30 requests foreground
//! ideal CPUs 1..7; 00DBDFE0 requests background priority -1 and CPUs 0..7.
//! Independent opt-ins retain the historical defaults for head A/B.
use super::*;
use std::sync::OnceLock;

pub(super) fn priority_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("DDRUNTIME_THREAD_PRIORITY").is_ok_and(|s| s == "1"))
}
pub(super) fn ideal_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("DDRUNTIME_THREAD_IDEAL").is_ok_and(|s| s == "1"))
}

pub(super) fn service(m: &mut dyn Mem, cpu: &mut Cpu, ideal: bool, enabled: bool) {
    cpu.eax = if !enabled {
        if ideal { 0 } else { 1 }
    } else {
        match apply(arg(m, cpu, 0), arg(m, cpu, 1), ideal) {
            Ok(value) => value,
            Err(error) => {
                set_error(error);
                if ideal { u32::MAX } else { 0 }
            }
        }
    };
    ret(m, cpu, 8);
}

#[cfg(windows)]
fn apply(handle: u32, value: u32, ideal: bool) -> Result<u32, u32> {
    use std::os::windows::io::AsRawHandle;
    // MAXIMUM_PROCESSORS is 32 for the guest x86 ABI, 64 for our x64 host.
    // It queries the old ideal CPU without changing it. Values above the
    // guest sentinel remain invalid even when the host could accept them.
    if ideal && value > 32 { return Err(87); }
    let value = if ideal && value == 32 { usize::BITS } else { value };
    let invoke = |host| unsafe {
        let result = if ideal { SetThreadIdealProcessor(host, value) }
                     else { SetThreadPriority(host, value as i32) as u32 };
        if (ideal && result == u32::MAX) || (!ideal && result == 0) {
            Err(GetLastError())
        } else {
            Ok(if ideal && result == usize::BITS { 32 } else { result })
        }
    };
    if handle == CURRENT_THREAD {
        return invoke(CURRENT_THREAD as i32 as isize as *mut std::ffi::c_void);
    }
    kernel(|k| {
        let object = k.object_of(handle).ok_or(ERROR_INVALID_HANDLE)?;
        let Some(Object::Thread { tid, .. }) = k.objects.get(&object) else {
            return Err(ERROR_INVALID_HANDLE);
        };
        let host = k.host_threads.get(tid).ok_or(ERROR_INVALID_HANDLE)?;
        invoke(host.as_raw_handle())
    })
}

#[cfg(not(windows))]
fn apply(_: u32, _: u32, _: bool) -> Result<u32, u32> { Err(50) }

// https://learn.microsoft.com/windows/win32/api/processthreadsapi/nf-processthreadsapi-setthreadpriority
// https://learn.microsoft.com/windows/win32/api/processthreadsapi/nf-processthreadsapi-setthreadidealprocessor
#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetThreadPriority(thread: *mut std::ffi::c_void, priority: i32) -> i32;
    fn SetThreadIdealProcessor(thread: *mut std::ffi::c_void, processor: u32) -> u32;
    fn GetLastError() -> u32;
    #[cfg(test)]
    fn GetThreadPriority(thread: *mut std::ffi::c_void) -> i32;
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;

    #[test]
    fn created_suspended_guest_observes_settings_before_first_callback() {
        fn worker(m: &mut dyn Mem, cpu: &mut Cpu) {
            m.set_u32(0x11000, unsafe { GetThreadPriority(-2isize as *mut _) } as u32);
            m.set_u32(0x11004, apply(CURRENT_THREAD, 32, true).unwrap());
            cpu.eax = 42;
            ret(m, cpu, 4);
        }
        fn call(m: &mut GuestMemory, f: fn(&mut dyn Mem, &mut Cpu), args: &[u32]) -> u32 {
            let mut cpu = Cpu { esp: 0x10000, ..Cpu::default() };
            m.set_u32(cpu.esp, 0);
            for (i, value) in args.iter().enumerate() {
                m.set_u32(cpu.esp + 4 + i as u32 * 4, *value);
            }
            f(m, &mut cpu);
            cpu.eax
        }
        for parallel_mode in [false, true] {
            runtime::enter(Arc::new(Runtime::default()));
            set_parallel(parallel_mode);
            let mut memory = GuestMemory::new();
            memory.map(0x10000, 0x2000, crate::memory::READ | crate::memory::WRITE).unwrap();
            memory.map_lazy(STACKS_BASE, STACKS_SIZE, crate::memory::READ | crate::memory::WRITE).unwrap();
            let mut registry = Registry::new();
            registry.insert(0x600000, crate::dispatch::Binding::Lifted(worker)).unwrap();
            set_machine(memory.share(), Arc::new(registry));
            let handle = call(&mut memory, create_thread, &[0, 0, 0x600000, 0, CREATE_SUSPENDED, 0]);
            assert_eq!(apply(handle, (-1i32) as u32, false), Ok(1));
            apply(handle, 0, true).unwrap();
            assert_eq!(memory.u32(0x11000), 0);
            assert_eq!(call(&mut memory, resume_thread, &[handle]), 1);
            assert_eq!(wait(&[handle], false, 5000), WAIT_OBJECT_0);
            assert_eq!(memory.u32(0x11000), (-1i32) as u32);
            assert_eq!(memory.u32(0x11004), 0);
            assert!(faults().is_empty());
            assert!(close(handle));
            assert!(kernel(|k| k.host_threads.is_empty()));
        }
    }

    #[test]
    fn original_background_priority_and_processor_hint_reach_host() {
        let rt = Arc::new(Runtime::default());
        runtime::enter(rt.clone());
        let (send, receive) = std::sync::mpsc::channel();
        let host = std::thread::spawn(move || { receive.recv().unwrap(); });
        let raw = host.as_raw_handle();
        let handle = kernel(|k| {
            let object = k.add(Object::Thread { tid: 123, exit: None });
            k.host_threads.insert(123, host);
            k.handle(object)
        });
        assert_eq!(apply(handle, (-1i32) as u32, false), Ok(1));
        assert_eq!(unsafe { GetThreadPriority(raw) }, -1);
        let previous = apply(handle, 32, true).unwrap();
        assert_eq!(apply(handle, 0, true), Ok(previous));
        assert_eq!(apply(handle, 32, true), Ok(0));
        assert_eq!(apply(handle, 33, true), Err(87));
        assert_eq!(apply(handle, 32, true), Ok(0));
        let alias = kernel(|k| k.handle(k.object_of(handle).unwrap()));
        assert!(close(handle));
        assert_eq!(apply(handle, 0, false), Err(ERROR_INVALID_HANDLE));
        assert_eq!(apply(alias, 0, false), Ok(1));
        send.send(()).unwrap();
        kernel(|k| k.host_threads.remove(&123)).unwrap().join().unwrap();
        assert!(close(alias));
    }

    #[test]
    fn current_thread_and_invalid_object_follow_win32_results() {
        // Use a dedicated thread so the test runner's own priority is unchanged.
        std::thread::spawn(|| {
            runtime::enter(Arc::new(Runtime::default()));
            assert_eq!(apply(CURRENT_THREAD, (-1i32) as u32, false), Ok(1));
            assert_eq!(unsafe { GetThreadPriority(-2isize as *mut _) }, -1);
            assert_eq!(apply(0, 0, false), Err(ERROR_INVALID_HANDLE));
            let event = kernel(|k| {
                let object = k.add(Object::Event(EventCell::new(false, false)));
                k.handle(object)
            });
            assert_eq!(apply(event, 0, true), Err(ERROR_INVALID_HANDLE));
        }).join().unwrap();
    }

    #[test]
    fn scheduling_service_preserves_stdcall_and_error_contract() {
        runtime::enter(Arc::new(Runtime::default()));
        let mut memory = GuestMemory::new();
        memory.map(0x10000, 0x1000, crate::memory::READ | crate::memory::WRITE).unwrap();
        for ideal in [false, true] {
            for enabled in [false, true] {
                let mut cpu = Cpu { esp: 0x10000, ..Cpu::default() };
                memory.set_u32(cpu.esp, 0x12345678);
                memory.set_u32(cpu.esp + 4, 0); // invalid handle
                memory.set_u32(cpu.esp + 8, 0);
                set_error(777);
                service(&mut memory, &mut cpu, ideal, enabled);
                assert_eq!(cpu.esp, 0x1000c);
                assert_eq!(cpu.eax, match (enabled, ideal) {
                    (true, true) => u32::MAX, (true, false) => 0,
                    (false, true) => 0, (false, false) => 1,
                });
                assert_eq!(runtime::last_error(), if enabled { ERROR_INVALID_HANDLE } else { 777 });
            }
        }
    }
}
