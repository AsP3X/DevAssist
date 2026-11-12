//! Put text on the operating-system clipboard.

pub fn set_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }
    #[cfg(windows)]
    {
        set_windows(text)
    }
    #[cfg(not(windows))]
    {
        set_osc52(text)
    }
}

#[cfg(windows)]
fn set_windows(text: &str) -> Result<(), String> {
    use std::ffi::c_void;

    unsafe extern "system" {
        fn OpenClipboard(hwnd: *mut c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(format: u32, mem: *mut c_void) -> *mut c_void;
        fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
        fn GlobalLock(mem: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(mem: *mut c_void) -> i32;
        fn GlobalFree(mem: *mut c_void) -> *mut c_void;
    }

    const CF_UNICODETEXT: u32 = 13;
    const GMEM_MOVEABLE: u32 = 0x0002;

    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * std::mem::size_of::<u16>();
    unsafe {
        let mut opened = false;
        for _ in 0..8 {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        if !opened {
            return Err("Clipboard is busy".to_string());
        }
        if EmptyClipboard() == 0 {
            CloseClipboard();
            return Err("Could not copy".to_string());
        }
        let mem = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if mem.is_null() {
            CloseClipboard();
            return Err("Could not copy".to_string());
        }
        let locked = GlobalLock(mem).cast::<u16>();
        if locked.is_null() {
            GlobalFree(mem);
            CloseClipboard();
            return Err("Could not copy".to_string());
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), locked, wide.len());
        GlobalUnlock(mem);
        // A successful SetClipboardData transfers ownership of `mem`.
        if SetClipboardData(CF_UNICODETEXT, mem).is_null() {
            GlobalFree(mem);
            CloseClipboard();
            return Err("Could not copy".to_string());
        }
        CloseClipboard();
        Ok(())
    }
}

#[cfg(not(windows))]
fn set_osc52(text: &str) -> Result<(), String> {
    use std::io::Write;
    let encoded = super::encode::base64(text.as_bytes());
    let mut out = std::io::stdout().lock();
    write!(out, "\u{1b}]52;c;{encoded}\u{1b}\\").map_err(|err| err.to_string())?;
    out.flush().map_err(|err| err.to_string())
}

#[cfg(all(test, windows))]
mod tests {
    use super::set_text;

    #[test]
    fn unicode_text_round_trips_when_the_clipboard_holds_only_text() {
        let _guard = match ClipboardGuard::save() {
            Some(guard) => guard,
            None => return,
        };
        let sample = "devassist ä 中";
        set_text(sample).unwrap();
        assert_eq!(read_text().as_deref(), Some(sample));
    }

    struct ClipboardGuard {
        previous: Option<String>,
    }

    impl ClipboardGuard {
        fn save() -> Option<Self> {
            if !only_text_or_empty() {
                return None;
            }
            Some(Self {
                previous: read_text(),
            })
        }
    }

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(text) => {
                    let _ = set_text(&text);
                }
                None => clear(),
            }
        }
    }

    fn only_text_or_empty() -> bool {
        const CF_UNICODETEXT: u32 = 13;
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) == 0 {
                return false;
            }
            let formats = CountClipboardFormats();
            let text = IsClipboardFormatAvailable(CF_UNICODETEXT) != 0;
            CloseClipboard();
            formats == 0 || (text && formats == 1)
        }
    }

    fn read_text() -> Option<String> {
        const CF_UNICODETEXT: u32 = 13;
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) == 0 {
                return None;
            }
            let handle = GetClipboardData(CF_UNICODETEXT);
            if handle.is_null() {
                CloseClipboard();
                return None;
            }
            let locked = GlobalLock(handle).cast::<u16>();
            if locked.is_null() {
                CloseClipboard();
                return None;
            }
            let mut len = 0usize;
            while *locked.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(locked, len));
            GlobalUnlock(handle);
            CloseClipboard();
            Some(text)
        }
    }

    fn clear() {
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                EmptyClipboard();
                CloseClipboard();
            }
        }
    }

    use std::ffi::c_void;
    unsafe extern "system" {
        fn OpenClipboard(hwnd: *mut c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn EmptyClipboard() -> i32;
        fn GetClipboardData(format: u32) -> *mut c_void;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
        fn CountClipboardFormats() -> i32;
        fn GlobalLock(mem: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(mem: *mut c_void) -> i32;
    }
}
