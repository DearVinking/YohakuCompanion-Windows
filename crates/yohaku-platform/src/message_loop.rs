use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::Arc;
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
    PeekMessageW, QS_ALLINPUT, TranslateMessage, WM_QUIT,
};

#[derive(Clone)]
pub(crate) struct MessageLoop {
    stop_event: Arc<OwnedHandle>,
}

impl MessageLoop {
    pub(crate) fn new() -> Self {
        // SAFETY: 创建无名称、不可继承的 manual-reset event；无借用的参数。
        let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        assert!(
            !handle.is_null(),
            "create monitor stop event: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: CreateEventW 返回的有效句柄在此转移所有权，只由 OwnedHandle 关闭。
        let stop_event = Arc::new(unsafe { OwnedHandle::from_raw_handle(handle) });
        Self { stop_event }
    }

    pub(crate) fn stop(&self) {
        // SAFETY: Arc 保证有效的 event 句柄在调用期间不会关闭。
        // manual-reset event 会记住先于消息循环启动的停止请求。
        unsafe { SetEvent(self.stop_event.as_raw_handle()) };
    }

    pub(crate) fn run(&self) {
        let handle = self.stop_event.as_raw_handle();
        // SAFETY: 句柄由 self 保活；MSG 在当前监控线程读取和派发。
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            loop {
                let result = MsgWaitForMultipleObjectsEx(
                    1,
                    &handle,
                    INFINITE,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                );
                if result == WAIT_OBJECT_0 {
                    break;
                }
                if result != WAIT_OBJECT_0 + 1 {
                    log::warn!(
                        "monitor message wait failed: {}",
                        std::io::Error::last_os_error()
                    );
                    break;
                }
                if PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    if msg.message == WM_QUIT {
                        break;
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
}
