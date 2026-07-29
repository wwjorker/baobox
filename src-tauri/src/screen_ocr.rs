use crate::err::{AppError, AppResult};
use serde::Serialize;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};

/// 截图取字
///
/// 前面的工具都是「有文件 → 处理文件」，这个不一样：源头是屏幕上的
/// 任意一块像素。所以要先把整个虚拟桌面抓下来交给前端画选区遮罩，
/// 用户框完再把那块区域裁出来送去 OCR。
///
/// 抓的是**虚拟桌面**而不是主显示器——多屏用户在副屏上框选是常态，
/// 只抓主屏会让这个功能在一半场景下失效。

#[derive(Serialize, Clone)]
pub struct ScreenShot {
    /// PNG 的 base64，直接喂给 <img>
    pub data_url: String,
    /// 虚拟桌面左上角在系统坐标系里的位置，可能是负数
    pub origin_x: i32,
    pub origin_y: i32,
    pub width: u32,
    pub height: u32,
}

/// 声明本进程按物理像素工作。
///
/// 不声明的话，系统会把坐标按缩放比例虚拟化：150% 缩放下 2560×1440 的屏幕
/// 只会报成 1707×960，抓出来的图分辨率打了折，OCR 识别率跟着掉。
fn ensure_dpi_aware() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    });
}

/// 抓取整个虚拟桌面
fn grab_virtual_screen() -> AppResult<(Vec<u8>, i32, i32, i32, i32)> {
    ensure_dpi_aware();
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if w <= 0 || h <= 0 {
            return Err(AppError::new("err.screenGrab"));
        }

        let screen_dc = GetDC(HWND(std::ptr::null_mut()));
        let mem_dc = CreateCompatibleDC(screen_dc);
        let bitmap = CreateCompatibleBitmap(screen_dc, w, h);
        let old = SelectObject(mem_dc, bitmap);

        let ok = BitBlt(mem_dc, 0, 0, w, h, screen_dc, x, y, SRCCOPY).is_ok();

        // 关键：GetDIBits 要求目标位图当前**不处于选中状态**，
        // 否则它照样返回成功，但拷出来的是一整片零——抓屏结果全黑。
        // 所以必须先把原来的对象换回去，再去取像素。
        SelectObject(mem_dc, old);

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                // 负高度 = 自上而下，省得再翻转一遍
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let got = GetDIBits(
            mem_dc,
            bitmap,
            0,
            h as u32,
            Some(buf.as_mut_ptr() as *mut _),
            &mut info,
            DIB_RGB_COLORS,
        );

        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(mem_dc);
        ReleaseDC(HWND(std::ptr::null_mut()), screen_dc);

        if !ok || got == 0 {
            return Err(AppError::new("err.screenGrab"));
        }
        Ok((buf, x, y, w, h))
    }
}

/// BGRA → RGBA。GDI 给的是 BGRA，直接当 RGBA 用会红蓝颠倒。
fn bgra_to_rgb(buf: &[u8], w: u32, h: u32) -> image::RgbImage {
    let mut img = image::RgbImage::new(w, h);
    for (i, px) in img.pixels_mut().enumerate() {
        let o = i * 4;
        *px = image::Rgb([buf[o + 2], buf[o + 1], buf[o]]);
    }
    img
}

#[tauri::command]
pub async fn capture_screen() -> Result<ScreenShot, AppError> {
    tauri::async_runtime::spawn_blocking(|| {
        let (buf, ox, oy, w, h) = grab_virtual_screen()?;
        let img = bgra_to_rgb(&buf, w as u32, h as u32);
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|e| AppError::unknown(e))?;
        let b64 = base64_encode(&png.into_inner());
        Ok(ScreenShot {
            data_url: format!("data:image/png;base64,{b64}"),
            origin_x: ox,
            origin_y: oy,
            width: w as u32,
            height: h as u32,
        })
    })
    .await
    .map_err(|e| AppError::unknown(e))?
}

/// 裁出选区并 OCR。坐标是虚拟桌面内的相对坐标。
#[tauri::command]
pub async fn ocr_region(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    lang: Option<String>,
) -> Result<String, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        if width < 4 || height < 4 {
            return Err(AppError::new("err.regionTooSmall"));
        }
        let (buf, _, _, w, h) = grab_virtual_screen()?;
        let full = bgra_to_rgb(&buf, w as u32, h as u32);
        let cw = width.min(w as u32 - x.min(w as u32));
        let ch = height.min(h as u32 - y.min(h as u32));
        let cropped = image::imageops::crop_imm(&full, x, y, cw, ch).to_image();

        // OCR 走文件路径（WinRT 的解码器需要 StorageFile），写进临时目录
        let tmp = std::env::temp_dir().join(format!("baobox_region_{}.png", std::process::id()));
        image::DynamicImage::ImageRgb8(cropped)
            .save(&tmp)
            .map_err(|e| AppError::unknown(e))?;
        let text = crate::ocr::recognize_with_lang(&tmp, lang.as_deref());
        let _ = std::fs::remove_file(&tmp);
        text
    })
    .await
    .map_err(|e| AppError::unknown(e))?
}

/// 把窗口恢复并真正抢回前台。
///
/// Tauri 的 `setFocus()` 在 Windows 上常常不起作用：系统禁止非前台进程
/// 随意抢焦点，除非它跟当前前台窗口共享输入队列。抓屏时我们先最小化了
/// 自己，恢复后窗口就卡在别人后面，用户还得去点任务栏。
///
/// 标准解法是临时把自己的线程挂到前台窗口的输入队列上，
/// 让系统认为这次抢焦点是「同一个输入上下文里的操作」，用完立刻解除。
#[tauri::command]
pub fn restore_and_focus(window: tauri::Window) {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let Ok(handle) = window.hwnd() else { return };
    let hwnd = HWND(handle.0 as _);

    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);

        let fg = GetForegroundWindow();
        if fg.0.is_null() || fg == hwnd {
            let _ = SetForegroundWindow(hwnd);
            return;
        }
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let me = GetCurrentThreadId();

        if fg_thread != me {
            let _ = AttachThreadInput(me, fg_thread, true);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(hwnd);
            // 一定要解除，否则两个线程的输入队列会一直绑在一起
            let _ = AttachThreadInput(me, fg_thread, false);
        } else {
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

/// 光标位置，用于遮罩层打开时把放大镜对准鼠标
#[tauri::command]
pub fn cursor_pos() -> (i32, i32) {
    unsafe {
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        (p.x, p.y)
    }
}

/// 供其他模块复用的 base64 编码
pub fn b64(data: &[u8]) -> String {
    base64_encode(data)
}

fn base64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}
