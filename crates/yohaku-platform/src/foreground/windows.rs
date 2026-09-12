//! Win32 前台监控：SetWinEventHook(EVENT_SYSTEM_FOREGROUND / EVENT_OBJECT_NAMECHANGE)
//! 专用线程消息循环；进程路径 → 显示名（版本资源 FileDescription）+ 窗口标题。

use super::{FocusSample, SharedSample, application_key_from_path, fallback_display_name};
use std::sync::Arc;
use std::sync::mpsc::Sender;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EVENT_OBJECT_NAMECHANGE, EVENT_SYSTEM_FOREGROUND, GetForegroundWindow,
    GetMessageW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, MSG,
    TranslateMessage, WINEVENT_OUTOFCONTEXT,
};

/// WinEventProc 通过 thread_local 访问本线程的采样上下文
/// （OUTOFCONTEXT 钩子回调仅在安装线程被派发）。
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
    HOOK_CONTEXT.with(|ctx| {
        if let Some(context) = ctx.borrow().as_ref() {
            let changed = read_foreground(&context.sample);
            if changed {
                let _ = context.notify.send(());
            }
        }
    });
}

/// 读取前台窗口并更新样本；返回内容是否变化。
fn read_foreground(sample: &SharedSample<FocusSample>) -> bool {
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

/// 版本资源 FileDescription（Windows 桌面应用的常规显示名来源）。
fn file_description(exe_path: &str) -> Option<String> {
    let wide: Vec<u16> = exe_path.encode_utf16().chain([0]).collect();
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
        // 先取 \VarFileInfo\Translation 的语言代码页，再取对应 FileDescription
        let mut trans_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut trans_len: u32 = 0;
        let query = |path: &[u16], out: &mut *mut core::ffi::c_void, out_len: &mut u32| -> bool {
            VerQueryValueW(data.as_ptr() as *const _, path.as_ptr(), out, out_len) != 0
        };
        let translation_path: Vec<u16> = "\\VarFileInfo\\Translation"
            .encode_utf16()
            .chain([0])
            .collect();
        if !query(&translation_path, &mut trans_ptr, &mut trans_len) || trans_len < 2 {
            return None;
        }
        let trans = trans_ptr as *const u16;
        let language = *trans;
        let code_page = *trans.add(1);
        let sub_path = format!(
            "\\StringFileInfo\\{:04x}{:04x}\\FileDescription",
            language, code_page
        );
        let sub_wide: Vec<u16> = sub_path.encode_utf16().chain([0]).collect();
        if !query(&sub_wide, &mut value, &mut value_len) || value_len == 0 {
            return None;
        }
        let slice = std::slice::from_raw_parts(value as *const u16, value_len as usize);
        let text = String::from_utf16_lossy(slice);
        let trimmed = text.trim_end_matches('\0').trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    }
}

pub struct ForegroundMonitor {
    stop_flag: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ForegroundMonitor {
    /// 启动监控线程。事件变化时更新 `sample` 并向 `tx` 发通知。
    pub fn spawn(tx: Sender<()>, sample: SharedSample<FocusSample>) -> Self {
        let stop_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_clone = stop_flag.clone();
        let thread = std::thread::Builder::new()
            .name("foreground-monitor".into())
            .spawn(move || {
                HOOK_CONTEXT.with(|ctx| {
                    *ctx.borrow_mut() = Some(HookContext { sample, notify: tx });
                });
                // 启动即采样一次
                HOOK_CONTEXT.with(|ctx| {
                    if let Some(context) = ctx.borrow().as_ref() {
                        if read_foreground(&context.sample) {
                            let _ = context.notify.send(());
                        }
                    }
                });
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
                    let mut msg: MSG = std::mem::zeroed();
                    // GetMessageW 返回 0 = WM_QUIT，-1 = 错误
                    while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                        if stop_clone.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }
                    }
                    let _ = hook;
                }
                HOOK_CONTEXT.with(|ctx| *ctx.borrow_mut() = None);
            })
            .expect("spawn foreground monitor thread");
        ForegroundMonitor {
            stop_flag,
            thread: Some(thread),
        }
    }

    pub fn stop(&mut self) {
        self.stop_flag
            .store(true, std::sync::atomic::Ordering::Relaxed);
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
