//! Windows 实现：message-only 窗口接收 WM_POWERBROADCAST 与
//! WM_WTSSESSION_CHANGE；WinRT NetworkInformation 监听网络恢复。

use super::SystemEvent;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PBT_APMRESUMEAUTOMATIC,
    PBT_APMSUSPEND, RegisterClassW, TranslateMessage, WNDCLASSW, WTS_SESSION_LOCK,
    WTS_SESSION_UNLOCK,
};

const WM_POWERBROADCAST: u32 = 0x0218;
const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
const HWND_MESSAGE: isize = -3;
/// message-only 窗口类名 "YCMonitor"（NUL 结尾）。
const CLASS_NAME: &[u16] = &[
    'Y' as u16, 'C' as u16, 'M' as u16, 'o' as u16, 'n' as u16, 'i' as u16, 't' as u16, 'o' as u16,
    'r' as u16, 0,
];

struct EventContext {
    tx: Sender<SystemEvent>,
}

thread_local! {
    static CONTEXT: std::cell::RefCell<Option<EventContext>> = const { std::cell::RefCell::new(None) };
}

/// 向协调器转发系统事件（线程上下文未就绪时静默丢弃）。
fn notify(event: SystemEvent) {
    CONTEXT.with(|c| {
        if let Some(ctx) = c.borrow().as_ref() {
            let _ = ctx.tx.send(event);
        }
    });
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_POWERBROADCAST {
        if wparam as u32 == PBT_APMSUSPEND {
            notify(SystemEvent::SleepOrLock);
        } else if wparam as u32 == PBT_APMRESUMEAUTOMATIC {
            notify(SystemEvent::Wake);
        }
        return 0;
    }
    if msg == WM_WTSSESSION_CHANGE {
        match wparam as u32 {
            WTS_SESSION_LOCK => notify(SystemEvent::SleepOrLock),
            WTS_SESSION_UNLOCK => notify(SystemEvent::Wake),
            _ => {}
        }
        return 0;
    }
    // SAFETY: 未处理的消息必须交给默认窗口过程；参数原样透传。
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

pub struct SystemEvents {
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SystemEvents {
    pub fn spawn(tx: Sender<SystemEvent>) -> Self {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_clone = stop.clone();
        let thread = std::thread::Builder::new()
            .name("system-events".into())
            .spawn(move || {
                CONTEXT.with(|c| *c.borrow_mut() = Some(EventContext { tx: tx.clone() }));
                // SAFETY: 类名/窗口名以 NUL 结尾；message-only 窗口的注册与
                // 创建都在本线程，消息循环随后在本线程运行。
                unsafe {
                    let wc = WNDCLASSW {
                        style: 0,
                        lpfnWndProc: Some(wnd_proc),
                        cbClsExtra: 0,
                        cbWndExtra: 0,
                        hInstance: std::ptr::null_mut(),
                        hIcon: std::ptr::null_mut(),
                        hCursor: std::ptr::null_mut(),
                        hbrBackground: std::ptr::null_mut(),
                        lpszMenuName: std::ptr::null(),
                        lpszClassName: CLASS_NAME.as_ptr(),
                    };
                    if RegisterClassW(&wc) == 0 {
                        return;
                    }
                    let hwnd = CreateWindowExW(
                        0,
                        CLASS_NAME.as_ptr(),
                        std::ptr::null(),
                        0,
                        0,
                        0,
                        0,
                        0,
                        HWND_MESSAGE as HWND,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null(),
                    );
                    if hwnd.is_null() {
                        return;
                    }
                    let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
                    // WinRT 网络监听（需要套间）
                    if windows::Win32::System::Com::CoInitializeEx(
                        None,
                        windows::Win32::System::Com::COINIT_MULTITHREADED,
                    )
                    .is_ok()
                    {
                        spawn_network_watcher(tx.clone());
                    }
                    let mut msg: MSG = std::mem::zeroed();
                    while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                        if stop_clone.load(Ordering::Relaxed) {
                            break;
                        }
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            })
            .expect("spawn system events thread");
        SystemEvents {
            stop,
            thread: Some(thread),
        }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for SystemEvents {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 网络恢复监听：连接级别回到 InternetAccess 时发一次 NetworkUp。
fn spawn_network_watcher(tx: Sender<SystemEvent>) {
    std::thread::Builder::new()
        .name("network-watcher".into())
        .spawn(move || {
            use windows::Networking::Connectivity::{NetworkConnectivityLevel, NetworkInformation};
            let (notify_tx, notify_rx) = std::sync::mpsc::channel::<()>();
            let handler = windows::Networking::Connectivity::NetworkStatusChangedEventHandler::new(
                move |_s: windows::core::Ref<'_, windows::core::IInspectable>| {
                    let _ = notify_tx.send(());
                    Ok(())
                },
            );
            if NetworkInformation::NetworkStatusChanged(&handler).is_err() {
                return;
            }
            let mut was_online = false;
            loop {
                // 唤醒即评估；NetworkStatusChanged 只触发 notify_rx
                if notify_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .is_err()
                {
                    // 超时也复查一次（保守）
                }
                let online = NetworkInformation::GetInternetConnectionProfile()
                    .map(|p| {
                        p.GetNetworkConnectivityLevel().ok()
                            == Some(NetworkConnectivityLevel::InternetAccess)
                    })
                    .unwrap_or(false);
                if online && !was_online {
                    let _ = tx.send(SystemEvent::NetworkUp);
                }
                was_online = online;
            }
        })
        .expect("spawn network watcher thread");
}
