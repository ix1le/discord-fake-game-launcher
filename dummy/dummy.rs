#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    type Ptr = *mut c_void;
    type WndProc = unsafe extern "system" fn(Ptr, u32, usize, isize) -> isize;

    #[repr(C)]
    struct WndClassW {
        style: u32,
        wnd_proc: Option<WndProc>,
        cls_extra: i32,
        wnd_extra: i32,
        instance: Ptr,
        icon: Ptr,
        cursor: Ptr,
        background: Ptr,
        menu_name: *const u16,
        class_name: *const u16,
    }

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct Msg {
        hwnd: Ptr,
        message: u32,
        w_param: usize,
        l_param: isize,
        time: u32,
        pt: Point,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn RegisterClassW(wc: *const WndClassW) -> u16;
        fn CreateWindowExW(
            ex_style: u32,
            class: *const u16,
            title: *const u16,
            style: u32,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            parent: Ptr,
            menu: Ptr,
            instance: Ptr,
            param: *const c_void,
        ) -> Ptr;
        fn ShowWindow(hwnd: Ptr, cmd: i32) -> i32;
        fn GetMessageW(msg: *mut Msg, hwnd: Ptr, min: u32, max: u32) -> i32;
        fn TranslateMessage(msg: *const Msg) -> i32;
        fn DispatchMessageW(msg: *const Msg) -> isize;
        fn DefWindowProcW(hwnd: Ptr, msg: u32, wp: usize, lp: isize) -> isize;
        fn PostQuitMessage(code: i32);
        fn LoadCursorW(instance: Ptr, name: *const u16) -> Ptr;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> Ptr;
    }

    const WM_DESTROY: u32 = 0x0002;
    const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
    const CW_USEDEFAULT: i32 = i32::MIN;
    const SW_SHOWMINNOACTIVE: i32 = 7;
    const IDC_ARROW: usize = 32512;
    const COLOR_WINDOW: usize = 5;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe extern "system" fn wnd_proc(hwnd: Ptr, msg: u32, wp: usize, lp: isize) -> isize {
        unsafe {
            if msg == WM_DESTROY {
                PostQuitMessage(0);
                return 0;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
    }

    pub fn run(title: &str) -> ! {
        unsafe {
            let instance = GetModuleHandleW(null());
            let class = wide("FakeGameWindow");
            let wc = WndClassW {
                style: 0,
                wnd_proc: Some(wnd_proc),
                cls_extra: 0,
                wnd_extra: 0,
                instance,
                icon: null_mut(),
                cursor: LoadCursorW(null_mut(), IDC_ARROW as *const u16),
                background: (COLOR_WINDOW + 1) as Ptr,
                menu_name: null(),
                class_name: class.as_ptr(),
            };
            RegisterClassW(&wc);

            let title = wide(title);
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                360,
                120,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_SHOWMINNOACTIVE);
            }

            let mut msg: Msg = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::process::exit(0)
    }
}

#[cfg(not(windows))]
mod win {
    pub fn run(title: &str) -> ! {
        println!("{title} (fake process for Discord)");
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let name = match args.iter().position(|a| a == "--dummy") {
        Some(i) => args.get(i + 1).cloned(),
        None => args.first().cloned(),
    }
    .unwrap_or_else(|| "Game".to_string());
    win::run(&name);
}
