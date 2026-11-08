//! A small wrapper over libmpv's OpenGL render API (raw C calls).
//!
//! jimmy writes this itself instead of using the libmpv2 crate's wrapper so it can pass a DRM
//! render node to mpv: that lets VA-API hand decoded frames straight to OpenGL under both X11
//! and Wayland, without mpv having to be built with its X11/Wayland window code.

use libmpv2_sys as sys;
use std::ffi::{CStr, c_char, c_int, c_void};
#[cfg(target_os = "linux")]
use std::ffi::CString;
use std::ptr;

pub type GetProcAddress = std::sync::Arc<dyn Fn(&CStr) -> *const c_void + Send + Sync>;

#[derive(Clone, Copy)]
#[allow(dead_code)] // constructed on Linux only
pub enum DisplayHandle {
    Wayland(*const c_void),
    X11(*const c_void),
}

unsafe extern "C" fn get_proc_address(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let gpa = unsafe { &*(ctx as *const GetProcAddress) };
    let name = unsafe { CStr::from_ptr(name) };
    gpa(name) as *mut c_void
}

unsafe extern "C" fn on_update(ctx: *mut c_void) {
    let cb = unsafe { &*(ctx as *const Box<dyn Fn() + Send>) };
    cb();
}

pub struct Renderer {
    ctx: *mut sys::mpv_render_context,
    gpa: *mut GetProcAddress,
    update: *mut Box<dyn Fn() + Send>,
    render_fd: c_int,
}

// Only used on the UI thread; the wrapper types around it need Send + Sync.
unsafe impl Send for Renderer {}
unsafe impl Sync for Renderer {}

/// First DRM render node (e.g. /dev/dri/renderD128), for VA-API interop. -1 if none.
fn open_render_node() -> c_int {
    #[cfg(target_os = "linux")]
    for n in 128..136 {
        let path = CString::new(format!("/dev/dri/renderD{n}")).unwrap();
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
        if fd >= 0 {
            return fd;
        }
    }
    -1
}

impl Renderer {
    pub fn new(mpv: *mut sys::mpv_handle, gpa: GetProcAddress, display: Option<DisplayHandle>, update: Box<dyn Fn() + Send>) -> anyhow::Result<Self> {
        let gpa = Box::into_raw(Box::new(gpa));
        let render_fd = open_render_node();
        let mut init = sys::mpv_opengl_init_params { get_proc_address: Some(get_proc_address), get_proc_address_ctx: gpa as *mut c_void };
        let mut drm = sys::mpv_opengl_drm_params_v2 { fd: -1, crtc_id: -1, connector_id: -1, atomic_request_ptr: ptr::null_mut(), render_fd };
        let api = c"opengl";
        let mut params = vec![
            sys::mpv_render_param { type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE, data: api.as_ptr() as *mut c_void },
            sys::mpv_render_param { type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS, data: &mut init as *mut _ as *mut c_void },
        ];
        // Native displays let mpv open VA-API for zero-copy frames; the render node works everywhere.
        match display {
            Some(DisplayHandle::X11(p)) => params.push(sys::mpv_render_param { type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_X11_DISPLAY, data: p as *mut c_void }),
            Some(DisplayHandle::Wayland(p)) => params.push(sys::mpv_render_param { type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_WL_DISPLAY, data: p as *mut c_void }),
            None => {}
        }
        if render_fd >= 0 {
            params.push(sys::mpv_render_param { type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_DRM_DISPLAY_V2, data: &mut drm as *mut _ as *mut c_void });
        }
        params.push(sys::mpv_render_param { type_: 0, data: ptr::null_mut() });

        let mut ctx: *mut sys::mpv_render_context = ptr::null_mut();
        let err = unsafe { sys::mpv_render_context_create(&mut ctx, mpv, params.as_mut_ptr()) };
        if err < 0 {
            unsafe {
                drop(Box::from_raw(gpa));
                if render_fd >= 0 {
                    libc::close(render_fd);
                }
            }
            anyhow::bail!("mpv render context: error {err}");
        }
        let update = Box::into_raw(Box::new(update));
        unsafe { sys::mpv_render_context_set_update_callback(ctx, Some(on_update), update as *mut c_void) };
        Ok(Renderer { ctx, gpa, update, render_fd })
    }

    /// Render the current frame into framebuffer `fbo` of size w×h (flipped for OpenGL).
    pub fn render(&self, fbo: i32, w: i32, h: i32) {
        let mut target = sys::mpv_opengl_fbo { fbo, w, h, internal_format: 0 };
        let mut flip: c_int = 1;
        let mut params = [
            sys::mpv_render_param { type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_FBO, data: &mut target as *mut _ as *mut c_void },
            sys::mpv_render_param { type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_FLIP_Y, data: &mut flip as *mut _ as *mut c_void },
            sys::mpv_render_param { type_: 0, data: ptr::null_mut() },
        ];
        let err = unsafe { sys::mpv_render_context_render(self.ctx, params.as_mut_ptr()) };
        if err < 0 {
            log::debug!("mpv render: error {err}");
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            sys::mpv_render_context_set_update_callback(self.ctx, None, ptr::null_mut());
            sys::mpv_render_context_free(self.ctx);
            drop(Box::from_raw(self.update));
            drop(Box::from_raw(self.gpa));
            if self.render_fd >= 0 {
                libc::close(self.render_fd);
            }
        }
    }
}
