//! Observe only mouse-button state and window geometry; no window titles,
//! screenshots, accessibility automation or permission prompts.
#[cfg(target_os = "macos")]
mod macos {
    use objc::{class, msg_send, runtime::Object, sel, sel_impl};
    use std::ffi::{c_char, c_void};
    #[repr(C)]
    struct Point {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    struct Size {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    struct Rect {
        origin: Point,
        size: Size,
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGWindowListCopyWindowInfo(options: u32, relative: u32) -> *const c_void;
        fn CGGetActiveDisplayList(max: u32, ids: *mut u32, count: *mut u32) -> i32;
        fn CGDisplayBounds(id: u32) -> Rect;
        fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
        static kCGWindowOwnerPID: *const c_void;
        static kCGWindowBounds: *const c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(value: *const c_void);
    }
    struct Owned(*const c_void);
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe {
                CFRelease(self.0);
            }
        }
    }
    unsafe fn number(dict: *mut Object, key: &'static [u8]) -> f64 {
        let key: *mut Object =
            msg_send![class!(NSString), stringWithUTF8String:key.as_ptr() as *const c_char];
        let value: *mut Object = msg_send![dict, objectForKey:key];
        if value.is_null() {
            return 0.0;
        }
        msg_send![value, doubleValue]
    }
    pub(super) fn mouse_down() -> bool {
        unsafe { CGEventSourceButtonState(0, 0) }
    }
    pub(super) fn fullscreen() -> bool {
        objc::rc::autoreleasepool(|| unsafe {
            let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
            let front: *mut Object = msg_send![workspace, frontmostApplication];
            if front.is_null() {
                return false;
            }
            let pid: i32 = msg_send![front, processIdentifier];
            let raw = CGWindowListCopyWindowInfo(1 | 16, 0);
            if raw.is_null() {
                return false;
            }
            let _owned = Owned(raw);
            let list = raw as *mut Object;
            let count: usize = msg_send![list, count];
            let mut ids = [0u32; 16];
            let mut displays = 0;
            if CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut displays) != 0 {
                return false;
            }
            for i in 0..count {
                let item: *mut Object = msg_send![list,objectAtIndex:i];
                let owner: *mut Object =
                    msg_send![item,objectForKey:kCGWindowOwnerPID as *mut Object];
                if owner.is_null() {
                    continue;
                }
                let owner_pid: i32 = msg_send![owner, intValue];
                if owner_pid != pid {
                    continue;
                }
                let bounds: *mut Object =
                    msg_send![item,objectForKey:kCGWindowBounds as *mut Object];
                if bounds.is_null() {
                    continue;
                }
                let x = number(bounds, b"X\0");
                let y = number(bounds, b"Y\0");
                let w = number(bounds, b"Width\0");
                let h = number(bounds, b"Height\0");
                for id in ids.iter().take(displays.min(16) as usize) {
                    let r = CGDisplayBounds(*id);
                    if (x - r.origin.x).abs() < 2.0
                        && (y - r.origin.y).abs() < 2.0
                        && (w - r.size.width).abs() < 2.0
                        && (h - r.size.height).abs() < 2.0
                    {
                        return true;
                    }
                }
            }
            false
        })
    }
}

pub(super) fn primary_button_down() -> bool {
    #[cfg(target_os = "macos")]
    {
        return macos::mouse_down();
    }
    #[cfg(target_os = "windows")]
    {
        #[link(name = "user32")]
        extern "system" {
            fn GetAsyncKeyState(key: i32) -> i16;
        }
        return unsafe { GetAsyncKeyState(1) < 0 };
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}
pub(super) fn foreground_fullscreen() -> bool {
    #[cfg(target_os = "macos")]
    {
        return macos::fullscreen();
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}
