use super::{FocusSample, SharedSample, application_key_from_path, fallback_display_name};
use crate::message_loop::MessageLoop;
use std::sync::mpsc::Sender;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EVENT_OBJECT_NAMECHANGE, EVENT_SYSTEM_FOREGROUND, GetForegroundWindow, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, WINEVENT_OUTOFCONTEXT,
};

struct HookContext {
    sample: SharedSample<FocusSample>,
    notify: Sender<()>,
}

thread_local! {
    static HOOK_CONTEXT: std::cell::RefCell<Option<HookContext>> = const { std::cell::RefCell::new(None) };
}

unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: windows_sys::Win32::Foundation::HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    sample_and_notify();
}

fn sample_and_notify() {
    HOOK_CONTEXT.with(|ctx| {
        if let Some(context) = ctx.borrow().as_ref()
            && read_foreground(&context.sample)
        {
            let _ = context.notify.send(());
        }
    });
}

fn read_foreground(sample: &SharedSample<FocusSample>) -> bool {
    // SAFETY: hwnd 来自 GetForegroundWindow 并做了非空检查；
    // 其余调用为只读 Win32 查询，句柄/缓冲区生命周期都在本函数内。
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return false;
        }
        let Some(exe_path) = process_image_path(pid) else {
            return false;
        };
        let application_key = application_key_from_path(&exe_path);
        let display_name =
            file_description(&exe_path).unwrap_or_else(|| fallback_display_name(&exe_path));
        let window_title = read_window_title(hwnd);
        let new_sample = FocusSample {
            application_key,
            display_name,
            window_title,
        };
        let mut guard = sample.lock().unwrap();
        let changed = guard.as_ref() != Some(&new_sample);
        if changed {
            *guard = Some(new_sample);
        }
        changed
    }
}

fn process_image_path(pid: u32) -> Option<String> {
    // SAFETY: process 句柄由 OpenProcess 成功返回；缓冲区容量与长度参数一致。
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(process as HANDLE);
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn read_window_title(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<String> {
    // SAFETY: hwnd 由调用方从 GetForegroundWindow 取得且有效；
    // buf 容量（len+1）与 GetWindowTextW 的长度参数一致。
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; (len as usize) + 1];
        let copied = GetWindowTextW(hwnd, buf.as_mut_ptr(), len + 1);
        if copied <= 0 {
            return None;
        }
        let title = String::from_utf16_lossy(&buf[..copied as usize]);
        (!title.is_empty()).then_some(title)
    }
}

unsafe fn translation_pair(value: *const core::ffi::c_void, byte_len: u32) -> Option<(u16, u16)> {
    if value.is_null() || (byte_len as usize) < std::mem::size_of::<[u16; 2]>() {
        return None;
    }
    // SAFETY: 调用方保活 VerQueryValueW 返回的缓冲区；其长度单位为字节。
    // 一个语言/代码页对占四字节；Vec<u8> 不保证 u16 对齐，故使用非对齐读取。
    let [language, code_page] = unsafe { value.cast::<[u16; 2]>().read_unaligned() };
    Some((language, code_page))
}

fn file_description(exe_path: &str) -> Option<String> {
    let wide: Vec<u16> = exe_path.encode_utf16().chain([0]).collect();
    // SAFETY: data 缓冲区按 GetFileVersionInfoSizeW 返回的大小分配；
    // 查询返回的指针在 data 存活时有效；translation 长度按字节检查，
    // 字符串长度按 UTF-16 单元使用。读取不依赖 Vec<u8> 的对齐方式。
    unsafe {
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), std::ptr::null_mut());
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, data.as_mut_ptr() as *mut _) == 0 {
            return None;
        }
        let mut value: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut value_len: u32 = 0;
        let mut trans_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut trans_len: u32 = 0;
        let query = |path: &[u16], out: &mut *mut core::ffi::c_void, out_len: &mut u32| -> bool {
            VerQueryValueW(data.as_ptr() as *const _, path.as_ptr(), out, out_len) != 0
        };
        let translation_path: Vec<u16> = "\\VarFileInfo\\Translation"
            .encode_utf16()
            .chain([0])
            .collect();
        if !query(&translation_path, &mut trans_ptr, &mut trans_len) {
            return None;
        }
        let (language, code_page) = translation_pair(trans_ptr, trans_len)?;
        let sub_path = format!(
            "\\StringFileInfo\\{:04x}{:04x}\\FileDescription",
            language, code_page
        );
        let sub_wide: Vec<u16> = sub_path.encode_utf16().chain([0]).collect();
        if !query(&sub_wide, &mut value, &mut value_len) || value.is_null() || value_len == 0 {
            return None;
        }
        let units: Vec<u16> = (0..value_len as usize)
            .map(|index| value.cast::<u16>().add(index).read_unaligned())
            .collect();
        let text = String::from_utf16_lossy(&units);
        let trimmed = text.trim_end_matches('\0').trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    }
}

#[must_use = "dropping the monitor stops foreground capture"]
pub struct ForegroundMonitor {
    message_loop: MessageLoop,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ForegroundMonitor {
    pub fn spawn(tx: Sender<()>, sample: SharedSample<FocusSample>) -> Self {
        let message_loop = MessageLoop::new();
        let worker_loop = message_loop.clone();
        let thread = std::thread::Builder::new()
            .name("foreground-monitor".into())
            .spawn(move || {
                HOOK_CONTEXT.with(|ctx| {
                    *ctx.borrow_mut() = Some(HookContext { sample, notify: tx });
                });
                sample_and_notify();
                // SAFETY: 本线程为专用消息循环线程；钩子回调（OUTOFCONTEXT）
                // 仅在本线程派发，HOOK_CONTEXT 已在上方初始化。
                unsafe {
                    let hook = SetWinEventHook(
                        EVENT_SYSTEM_FOREGROUND,
                        EVENT_OBJECT_NAMECHANGE,
                        GetModuleHandleW(std::ptr::null()),
                        Some(win_event_proc),
                        0,
                        0,
                        WINEVENT_OUTOFCONTEXT,
                    );
                    worker_loop.run();
                    if !hook.is_null() {
                        UnhookWinEvent(hook);
                    }
                }
                HOOK_CONTEXT.with(|ctx| *ctx.borrow_mut() = None);
            })
            .expect("spawn foreground monitor thread");
        ForegroundMonitor {
            message_loop,
            thread: Some(thread),
        }
    }

    pub fn stop(&mut self) {
        self.message_loop.stop();
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ForegroundMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_pair_rejects_truncated_byte_counts() {
        let pair = [0x0409u16, 0x04b0];
        for byte_len in 0..4 {
            // SAFETY: the allocation contains both u16s even when the reported size is shorter.
            let value = unsafe { translation_pair(pair.as_ptr().cast(), byte_len) };
            assert_eq!(value, None);
        }
    }

    #[test]
    fn translation_pair_reads_unaligned_resource_bytes() {
        #[repr(align(2))]
        struct Resource([u8; 5]);
        let resource = Resource([0xaa, 0x09, 0x04, 0xb0, 0x04]);
        let value = resource.0.as_ptr().wrapping_add(1).cast();
        // SAFETY: the pointer covers four valid bytes and deliberately has odd alignment.
        let pair = unsafe { translation_pair(value, 4) };
        assert_eq!(pair, Some((0x0409, 0x04b0)));
    }

    #[test]
    fn translation_pair_rejects_null_output() {
        // SAFETY: the null output must be rejected before any read.
        assert_eq!(unsafe { translation_pair(std::ptr::null(), 4) }, None);
    }
}
