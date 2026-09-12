//! Win32 图标提取：SHGetFileInfoW → HICON → GetIconInfo/GetDIBits → RGBA → PNG。

use windows_sys::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits, HBITMAP, HDC,
    HGDIOBJ, ReleaseDC,
};
use windows_sys::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW};
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON};

/// 提取 exe 的 32×32 图标并编码为 PNG（BGRA → RGBA）。
pub fn extract_png(exe_path: &str) -> Option<Vec<u8>> {
    let wide: Vec<u16> = exe_path.encode_utf16().chain([0]).collect();
    // SAFETY: wide 以 NUL 结尾；hIcon 由 SHGetFileInfoW 成功返回，
    // 在销毁前交给 icon_to_png 使用。
    unsafe {
        let mut info: SHFILEINFOW = std::mem::zeroed();
        let ok = SHGetFileInfoW(
            wide.as_ptr(),
            0,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if ok == 0 || info.hIcon.is_null() {
            return None;
        }
        let result = icon_to_png(info.hIcon);
        DestroyIcon(info.hIcon);
        result
    }
}

/// # Safety
/// `hicon` 必须是有效的图标句柄。
unsafe fn icon_to_png(hicon: HICON) -> Option<Vec<u8>> {
    // SAFETY: 调用方保证 hicon 有效；GDI 位图句柄在失败分支统一清理，
    // 指针/长度参数均与缓冲区实际容量匹配。
    unsafe {
        let mut icon_info: windows_sys::Win32::UI::WindowsAndMessaging::ICONINFO =
            std::mem::zeroed();
        if GetIconInfo(hicon, &mut icon_info) == 0 {
            return None;
        }
        let cleanup = |info: &windows_sys::Win32::UI::WindowsAndMessaging::ICONINFO| {
            if !info.hbmMask.is_null() {
                DeleteObject(info.hbmMask as HGDIOBJ);
            }
            if !info.hbmColor.is_null() {
                DeleteObject(info.hbmColor as HGDIOBJ);
            }
        };
        if icon_info.hbmColor.is_null() {
            cleanup(&icon_info);
            return None;
        }
        let hdc: HDC = GetDC(std::ptr::null_mut());
        let mut probe: BITMAPINFO = std::mem::zeroed();
        probe.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        if GetDIBits(
            hdc,
            icon_info.hbmColor as HBITMAP,
            0,
            0,
            std::ptr::null_mut(),
            &mut probe,
            DIB_RGB_COLORS,
        ) == 0
        {
            cleanup(&icon_info);
            ReleaseDC(std::ptr::null_mut(), hdc);
            return None;
        }
        let width = probe.bmiHeader.biWidth.unsigned_abs();
        let height = probe.bmiHeader.biHeight.unsigned_abs();
        if width == 0 || height == 0 || width > 512 || height > 512 {
            cleanup(&icon_info);
            ReleaseDC(std::ptr::null_mut(), hdc);
            return None;
        }
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader = probe.bmiHeader;
        bmi.bmiHeader.biWidth = width as i32;
        bmi.bmiHeader.biHeight = -(height as i32); // top-down
        bmi.bmiHeader.biBitCount = 32;
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let lines = GetDIBits(
            hdc,
            icon_info.hbmColor as HBITMAP,
            0,
            height,
            pixels.as_mut_ptr() as *mut _,
            &mut bmi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(std::ptr::null_mut(), hdc);
        cleanup(&icon_info);
        if lines == 0 {
            return None;
        }
        // BGRA → RGBA
        for px in pixels.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        let img = image::RgbaImage::from_raw(width, height, pixels)?;
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .ok()?;
        Some(out)
    }
}
