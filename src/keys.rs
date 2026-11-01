#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::ptr::null_mut;

    type Ptr = *mut c_void;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> Ptr;
        fn GetWindowThreadProcessId(hwnd: Ptr, pid: *mut u32) -> u32;
        fn GetKeyboardLayout(thread_id: u32) -> Ptr;
        fn GetKeyboardLayoutList(count: i32, list: *mut Ptr) -> i32;
        fn VkKeyScanExW(ch: u16, hkl: Ptr) -> i16;
        fn GetAsyncKeyState(vk: i32) -> i16;
    }

    const VK_CONTROL: i32 = 0x11;
    const VK_RMENU: i32 = 0xA5;

    fn down(vk: i32) -> bool {
        unsafe { (GetAsyncKeyState(vk) as u16) & 0x8000 != 0 }
    }

    pub fn ctrl_held() -> bool {
        down(VK_CONTROL) && !down(VK_RMENU)
    }

    pub fn latin_letter(c: char) -> char {
        let lc = c.to_lowercase().next().unwrap_or(c);
        if lc.is_ascii() {
            return lc;
        }
        let mut buf = [0u16; 2];
        if lc.encode_utf16(&mut buf).len() != 1 {
            return lc;
        }
        let ch = buf[0];

        unsafe {
            let mut layouts: Vec<Ptr> = Vec::new();
            let mut pid = 0u32;
            let tid = GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
            let active = GetKeyboardLayout(tid);
            if !active.is_null() {
                layouts.push(active);
            }
            let n = GetKeyboardLayoutList(0, null_mut());
            if n > 0 {
                let mut list: Vec<Ptr> = vec![null_mut(); n as usize];
                let got = GetKeyboardLayoutList(n, list.as_mut_ptr());
                list.truncate(got.max(0) as usize);
                layouts.extend(list);
            }

            for hkl in layouts {
                let r = VkKeyScanExW(ch, hkl);
                if r == -1 {
                    continue;
                }
                let vk = (r & 0xFF) as u8;
                let shift_state = ((r >> 8) & 0xFF) as u8;

                if shift_state & 0b110 != 0 {
                    continue;
                }
                if (0x41..=0x5A).contains(&vk) {
                    return (vk as char).to_ascii_lowercase();
                }
            }
        }
        lc
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn ctrl_held() -> bool {
        false
    }

    pub fn latin_letter(c: char) -> char {
        c.to_lowercase().next().unwrap_or(c)
    }
}

pub use imp::{ctrl_held, latin_letter};
