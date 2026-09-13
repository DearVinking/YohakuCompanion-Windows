use std::marker::PhantomData;
use std::rc::Rc;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows_sys::Win32::Foundation::RPC_E_CHANGED_MODE;

pub(crate) struct ComApartment {
    initialized: bool,
    _thread: PhantomData<Rc<()>>,
}

impl ComApartment {
    pub(crate) fn initialize_mta() -> Result<Self, String> {
        // SAFETY: reserved 参数为空；套间仅供当前工作线程使用。
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_err() && result.0 != RPC_E_CHANGED_MODE {
            return Err(format!("initialize COM apartment: {result}"));
        }
        Ok(Self {
            initialized: result.is_ok(),
            _thread: PhantomData,
        })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: 本守卫不能跨线程；只平衡成功的初始化（包括 S_FALSE）。
            // RPC_E_CHANGED_MODE 未增加引用，不调用 CoUninitialize。
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::COINIT_APARTMENTTHREADED;

    fn assert_thread_can_initialize_sta() {
        // SAFETY: this test uses a fresh thread and balances every successful call.
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result.is_ok() {
            // SAFETY: the preceding call initialized this thread successfully.
            unsafe { CoUninitialize() };
        }
        assert!(
            result.is_ok(),
            "MTA initialization was not released: {result}"
        );
    }

    #[test]
    fn successful_initialization_is_released_on_the_same_thread() {
        std::thread::spawn(|| {
            let apartment = ComApartment::initialize_mta().unwrap();
            drop(apartment);
            assert_thread_can_initialize_sta();
        })
        .join()
        .unwrap();
    }

    #[test]
    fn repeated_successful_initialization_balances_s_false() {
        std::thread::spawn(|| {
            // SAFETY: this is a fresh worker thread; the successful call is balanced below.
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .unwrap();
            let apartment = ComApartment::initialize_mta().unwrap();
            drop(apartment);
            // SAFETY: releases the test's original successful initialization only.
            unsafe { CoUninitialize() };
            assert_thread_can_initialize_sta();
        })
        .join()
        .unwrap();
    }

    #[test]
    fn changed_mode_keeps_the_existing_apartment_alive() {
        std::thread::spawn(|| {
            // SAFETY: this is a fresh worker thread; the successful call is balanced below.
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
                .ok()
                .unwrap();
            let apartment = ComApartment::initialize_mta().unwrap();
            drop(apartment);
            // SAFETY: probing the existing STA with MTA must fail without adding a COM reference.
            let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            // SAFETY: releases the test's original successful STA initialization.
            unsafe { CoUninitialize() };
            assert_eq!(result.0, RPC_E_CHANGED_MODE);
        })
        .join()
        .unwrap();
    }
}
