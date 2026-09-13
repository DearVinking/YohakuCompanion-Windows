use super::SystemEvent;
use crate::com::ComApartment;
use crate::message_loop::MessageLoop;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use windows::Networking::Connectivity::{
    NetworkConnectivityLevel, NetworkInformation, NetworkStatusChangedEventHandler,
};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND,
    RegisterClassW, UnregisterClassW, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_POPUP, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};

struct EventContext {
    tx: Sender<SystemEvent>,
}

thread_local! {
    static CONTEXT: std::cell::RefCell<Option<EventContext>> = const { std::cell::RefCell::new(None) };
}

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
        return 1;
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

#[must_use = "dropping the monitor stops system event capture"]
pub struct SystemEvents {
    message_loop: MessageLoop,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SystemEvents {
    pub fn spawn(tx: Sender<SystemEvent>) -> Self {
        let message_loop = MessageLoop::new();
        let worker_loop = message_loop.clone();
        let thread = std::thread::Builder::new()
            .name("system-events".into())
            .spawn(move || {
                CONTEXT.with(|c| *c.borrow_mut() = Some(EventContext { tx: tx.clone() }));
                // SAFETY: 类名以 NUL 结尾；隐藏顶层窗口的注册与
                // 创建都在本线程，消息循环随后在本线程运行。
                unsafe {
                    let class_name: Vec<u16> = format!("YCMonitor-{}", GetCurrentThreadId())
                        .encode_utf16()
                        .chain([0])
                        .collect();
                    let instance = GetModuleHandleW(std::ptr::null());
                    let wc = WNDCLASSW {
                        style: 0,
                        lpfnWndProc: Some(wnd_proc),
                        cbClsExtra: 0,
                        cbWndExtra: 0,
                        hInstance: instance,
                        hIcon: std::ptr::null_mut(),
                        hCursor: std::ptr::null_mut(),
                        hbrBackground: std::ptr::null_mut(),
                        lpszMenuName: std::ptr::null(),
                        lpszClassName: class_name.as_ptr(),
                    };
                    if RegisterClassW(&wc) == 0 {
                        return;
                    }
                    let hwnd = CreateWindowExW(
                        WS_EX_TOOLWINDOW,
                        class_name.as_ptr(),
                        std::ptr::null(),
                        WS_POPUP,
                        0,
                        0,
                        0,
                        0,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        instance,
                        std::ptr::null(),
                    );
                    if hwnd.is_null() {
                        UnregisterClassW(class_name.as_ptr(), instance);
                        return;
                    }
                    let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
                    let network_watcher = NetworkWatcher::spawn(tx.clone()).ok();
                    worker_loop.run();
                    drop(network_watcher);
                    WTSUnRegisterSessionNotification(hwnd);
                    DestroyWindow(hwnd);
                    UnregisterClassW(class_name.as_ptr(), instance);
                }
                CONTEXT.with(|c| *c.borrow_mut() = None);
            })
            .expect("spawn system events thread");
        SystemEvents {
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

impl Drop for SystemEvents {
    fn drop(&mut self) {
        self.stop();
    }
}

struct NetworkRegistration(i64);

impl Drop for NetworkRegistration {
    fn drop(&mut self) {
        let _ = NetworkInformation::RemoveNetworkStatusChanged(self.0);
    }
}

struct NetworkWatcher {
    stop: Arc<AtomicBool>,
    wake: Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl NetworkWatcher {
    fn spawn(tx: Sender<SystemEvent>) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (wake, notify_rx) = mpsc::channel();
        let notify_tx = wake.clone();
        let thread = std::thread::Builder::new()
            .name("network-watcher".into())
            .spawn(move || {
                let Ok(_apartment) = ComApartment::initialize_mta() else {
                    return;
                };
                let handler = NetworkStatusChangedEventHandler::new(move |_| {
                    let _ = notify_tx.send(());
                    Ok(())
                });
                let Ok(_registration) =
                    NetworkInformation::NetworkStatusChanged(&handler).map(NetworkRegistration)
                else {
                    return;
                };
                let mut was_online = false;
                while !worker_stop.load(Ordering::Relaxed) {
                    if let Err(mpsc::RecvTimeoutError::Disconnected) =
                        notify_rx.recv_timeout(std::time::Duration::from_secs(5))
                    {
                        break;
                    }
                    if worker_stop.load(Ordering::Relaxed) {
                        break;
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
            })?;
        Ok(Self {
            stop,
            wake,
            thread: Some(thread),
        })
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.wake.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for NetworkWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::System::Threading::GetThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, IsWindowVisible, SMTO_ABORTIFHUNG, SendMessageTimeoutW,
    };

    fn top_level_window(monitor: &SystemEvents) -> HWND {
        unsafe extern "system" fn remember_window(hwnd: HWND, context: LPARAM) -> i32 {
            // SAFETY: EnumThreadWindows receives the address of the live HWND below.
            unsafe { *(context as *mut HWND) = hwnd };
            0
        }

        // SAFETY: the JoinHandle keeps the worker thread handle alive during the query.
        let thread_id = unsafe { GetThreadId(monitor.thread.as_ref().unwrap().as_raw_handle()) };
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let mut hwnd: HWND = std::ptr::null_mut();
            // SAFETY: the callback only writes to this stack value during enumeration.
            unsafe {
                EnumThreadWindows(
                    thread_id,
                    Some(remember_window),
                    &mut hwnd as *mut HWND as LPARAM,
                );
            }
            if !hwnd.is_null() {
                return hwnd;
            }
            assert!(
                Instant::now() < deadline,
                "system monitor has no top-level broadcast window"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn send_notification(hwnd: HWND, message: u32, event: u32) -> usize {
        let mut result = 0;
        // SAFETY: the monitor owns hwnd; these notifications carry only integer values.
        let sent = unsafe {
            SendMessageTimeoutW(
                hwnd,
                message,
                event as WPARAM,
                0,
                SMTO_ABORTIFHUNG,
                1_000,
                &mut result,
            )
        };
        assert_ne!(sent, 0, "system notification was not dispatched");
        result
    }

    fn assert_event(rx: &mpsc::Receiver<SystemEvent>, expected: SystemEvent) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let event = rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            if event != SystemEvent::NetworkUp {
                assert_eq!(event, expected);
                return;
            }
        }
    }

    #[test]
    fn hidden_top_level_window_forwards_power_and_session_notifications() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = SystemEvents::spawn(tx);
        let hwnd = top_level_window(&monitor);
        // SAFETY: the monitor keeps its window alive until stop below.
        assert_eq!(unsafe { IsWindowVisible(hwnd) }, 0);

        assert_eq!(
            send_notification(hwnd, WM_POWERBROADCAST, PBT_APMSUSPEND),
            1
        );
        assert_event(&rx, SystemEvent::SleepOrLock);
        assert_eq!(
            send_notification(hwnd, WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC),
            1
        );
        assert_event(&rx, SystemEvent::Wake);
        send_notification(hwnd, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK);
        assert_event(&rx, SystemEvent::SleepOrLock);
        send_notification(hwnd, WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK);
        assert_event(&rx, SystemEvent::Wake);
        monitor.stop();
    }

    #[test]
    fn stop_releases_every_system_event_sender() {
        for _ in 0..3 {
            let (tx, rx) = mpsc::channel();
            let mut monitor = SystemEvents::spawn(tx);
            monitor.stop();
            monitor.stop();
            while rx.try_recv().is_ok() {}
            assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Disconnected));
        }
    }

    #[test]
    fn network_registration_releases_its_callback() {
        std::thread::spawn(|| {
            use windows::Networking::Connectivity::{
                NetworkInformation, NetworkStatusChangedEventHandler,
            };

            let _apartment = crate::com::ComApartment::initialize_mta().unwrap();
            let lifetime = std::sync::Arc::new(());
            let callback_lifetime = std::sync::Arc::downgrade(&lifetime);
            let handler = NetworkStatusChangedEventHandler::new(move |_| {
                let _ = &lifetime;
                Ok(())
            });
            let registration =
                NetworkRegistration(NetworkInformation::NetworkStatusChanged(&handler).unwrap());
            drop(handler);
            assert!(callback_lifetime.upgrade().is_some());
            drop(registration);
            let deadline = Instant::now() + Duration::from_secs(2);
            while callback_lifetime.upgrade().is_some() {
                assert!(
                    Instant::now() < deadline,
                    "network callback remains registered after stop"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        })
        .join()
        .unwrap();
    }

    #[test]
    fn simultaneous_monitors_own_independent_windows() {
        let (first_tx, _first_rx) = mpsc::channel();
        let (second_tx, second_rx) = mpsc::channel();
        let mut first = SystemEvents::spawn(first_tx);
        let first_window = top_level_window(&first);
        let mut second = SystemEvents::spawn(second_tx);
        let second_window = top_level_window(&second);
        assert_ne!(first_window, second_window);
        first.stop();
        assert_eq!(
            send_notification(second_window, WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC),
            1
        );
        assert_event(&second_rx, SystemEvent::Wake);
        second.stop();
    }
}
