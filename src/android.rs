use std::ffi::{c_void, CString};
use std::ptr;

use gegl::EGLint;
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

use crate::{GlConfig, GlError};

use glutin_egl_sys::{self as gegl, egl, egl::types::*};

pub struct GlContext {
    display: EGLDisplay,
    context: EGLContext,
    surface: EGLSurface,
}
impl GlContext {
    pub unsafe fn create(
        parent: &impl HasRawWindowHandle,
        conf: GlConfig,
    ) -> Result<GlContext, GlError> {
        match conf.api {
            crate::Api::Gl => return Err(GlError::ApiNotSupported),
            crate::Api::Gles => {}
        }

        let handle = if let RawWindowHandle::AndroidNdk(handle) = parent.raw_window_handle() {
            handle
        } else {
            log::error!("invalid window handle: {:?}", parent.raw_window_handle());
            return Err(GlError::InvalidWindowHandle);
        };

        if handle.a_native_window.is_null() {
            log::error!("window handle is null");
            return Err(GlError::InvalidWindowHandle);
        }

        #[rustfmt::skip]
        let attribs = [
            egl::SURFACE_TYPE, egl::WINDOW_BIT as EGLenum,
            egl::RENDERABLE_TYPE, egl::OPENGL_ES2_BIT as EGLenum,
            egl::CONFORMANT, egl::OPENGL_ES2_BIT as EGLenum,
            egl::RED_SIZE, conf.red_bits as EGLenum,
            egl::GREEN_SIZE, conf.green_bits as EGLenum,
            egl::BLUE_SIZE, conf.blue_bits as EGLenum,
            egl::ALPHA_SIZE, conf.alpha_bits as EGLenum,
            egl::DEPTH_SIZE, conf.depth_bits as EGLenum,
            egl::STENCIL_SIZE, conf.stencil_bits as EGLenum,
            // egl::DOUBLEBUFFER, config.double_buffer as EGLenum,
            egl::SAMPLE_BUFFERS, conf.samples.is_some() as EGLenum,
            egl::SAMPLES, conf.samples.unwrap_or(0) as EGLenum,
            egl::NONE,
        ];

        let mut this = GlContext {
            display: egl::NO_DISPLAY,
            context: egl::NO_CONTEXT,
            surface: egl::NO_SURFACE,
        };

        let egl = egl::Egl;

        this.display = egl.GetDisplay(egl::DEFAULT_DISPLAY as *const _);
        if this.display == egl::NO_DISPLAY {
            log::error!("eglGetDisplay return NO_DISPLAY");
            return Err(GlError::CreationFailed);
        }

        let mut major = 0;
        let mut minor = 0;
        if egl.Initialize(this.display, &mut major, &mut minor) == egl::FALSE {
            log::error!("eglInitialize failed: {}", egl.GetError());
            return Err(GlError::CreationFailed);
        }

        log::info!("initialized EGL: version {}.{}", major, minor);

        let mut config: [EGLConfig; 64] = [ptr::null(); 64];
        let mut num_config: EGLint = 0;
        if egl.ChooseConfig(
            this.display,
            attribs.as_ptr() as *const EGLint,
            config.as_mut_ptr(),
            64,
            &mut num_config,
        ) == egl::FALSE
        {
            log::error!("eglChooseConfig failed: {}", egl.GetError());
            return Err(GlError::CreationFailed);
        }

        if num_config == 0 {
            log::error!("eglChooseConfig returned 0 configs");
            return Err(GlError::CreationFailed);
        }
        log::info!("eglChooseConfig returned {} configs", num_config);

        let window = handle.a_native_window;

        let mut configs = config[..num_config as usize].iter();
        let mut config: EGLConfig;
        loop {
            config = match configs.next() {
                Some(x) => *x,
                None => {
                    log::error!("all configs failed");
                    return Err(GlError::CreationFailed);
                }
            };

            this.surface = egl.CreateWindowSurface(this.display, config, window, ptr::null());

            if this.surface == egl::NO_SURFACE {
                let error = egl.GetError();
                log::error!(
                    "eglCreateWindowSurface failed: {} ({})",
                    match error as _ {
                        egl::BAD_DISPLAY => "EGL_BAD_DISPLAY",
                        egl::NOT_INITIALIZED => "EGL_NOT_INITIALIZED",
                        egl::BAD_CONFIG => "EGL_BAD_CONFIG",
                        egl::BAD_NATIVE_WINDOW => "EGL_BAD_NATIVE_WINDOW",
                        egl::BAD_ATTRIBUTE => "EGL_BAD_ATTRIBUTE",
                        egl::BAD_ALLOC => "EGL_BAD_ALLOC",
                        egl::BAD_MATCH => "EGL_BAD_MATCH",
                        _ => "Other",
                    },
                    error
                );
                continue;
            }

            break;
        }

        #[rustfmt::skip]
        let ctx_attribs = [ 
            // request a context using Open GL ES 2.0
            egl::CONTEXT_MAJOR_VERSION, conf.version.0 as EGLenum, 
            egl::CONTEXT_MINOR_VERSION, conf.version.1 as EGLenum, 
            egl::NONE 
        ];

        let shared_context = conf
            .share
            .map(|x| x.context.context)
            .unwrap_or(egl::NO_CONTEXT);

        this.context = egl.CreateContext(this.display, config, shared_context, ctx_attribs.as_ptr() as *const EGLint);
        if this.context == egl::NO_CONTEXT {
            log::error!("eglCreateContext failed: {}", egl.GetError());
            return Err(GlError::CreationFailed);
        }

        this.make_current();

        Ok(this)
    }

    pub unsafe fn make_current(&self) {
        let egl = egl::Egl;

        if egl.MakeCurrent(self.display, self.surface, self.surface, self.context) == egl::FALSE {
            log::error!("eglMakeCurrent failed in make_current: {}", egl.GetError());
        }
    }

    pub unsafe fn make_not_current(&self) {
        let egl = egl::Egl;

        if egl.MakeCurrent(self.display, egl::NO_SURFACE, egl::NO_SURFACE, egl::NO_CONTEXT) == egl::FALSE {
            log::error!(
                "eglMakeCurrent failed in make_not_current: {}",
                egl.GetError()
            );
        }
    }

    pub fn get_proc_address(&self, symbol: &str) -> *const c_void {
        let egl = egl::Egl;

        let symbol = CString::new(symbol).unwrap();
        unsafe { egl.GetProcAddress(symbol.as_ptr()) as *const c_void }
    }

    pub fn swap_buffers(&self) {
        let egl = egl::Egl;
        unsafe {
            if egl.SwapBuffers(self.display, self.surface) == egl::FALSE {
                let error = egl.GetError();
                log::error!(
                    "eglSwapBuffers failed in swap_buffer: {} ({})",
                    match error as _ {
                        egl::BAD_DISPLAY => "EGL_BAD_DISPLAY",
                        egl::NOT_INITIALIZED => "EGL_NOT_INITIALIZED",
                        egl::BAD_SURFACE => "EGL_BAD_SURFACE",
                        egl::CONTEXT_LOST => "EGL_CONTEXT_LOST",
                        _ => "Other",
                    },
                    error
                );
            }
        }
    }
}

impl Drop for GlContext {
    fn drop(&mut self) {
        log::info!("Destroying android GlContext");
        let egl = egl::Egl;
        if self.display != egl::NO_DISPLAY {
            unsafe {
                self.make_not_current();

                if self.surface != egl::NO_SURFACE {

                    log::debug!("eglDestroySurface");
                    egl.DestroySurface(self.display, self.surface);
                    self.surface = egl::NO_SURFACE;
                }

                if self.context != egl::NO_CONTEXT {
                    log::debug!("eglDestroyContext");
                    egl.DestroyContext(self.display, self.context);
                }
                log::debug!("eglTerminate");
                egl.Terminate(self.display);
            }
        }
    }
}
