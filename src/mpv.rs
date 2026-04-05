// Runtime loading libmpv через dlopen.
// Не требует libmpv-dev для компиляции — библиотека загружается в рантайме.
//
// Значения констант взяты из mpv/render.h и mpv/client.h.

use libloading::{Library, Symbol};
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};

/// Соответствует mpv_opengl_fbo из render_gl.h
#[repr(C)]
pub struct MpvOpenGLFbo {
    pub fbo: c_int,
    pub w: c_int,
    pub h: c_int,
    pub internal_format: c_int,
}

/// Соответствует mpv_render_param из render.h
#[repr(C)]
pub struct MpvRenderParam {
    pub type_: u32,
    pub data: *mut c_void,
}

// Константы из mpv/render.h
pub const MPV_RENDER_PARAM_INVALID: u32 = 0;
pub const MPV_RENDER_PARAM_API_TYPE: u32 = 1;
pub const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: u32 = 2;
pub const MPV_RENDER_PARAM_OPENGL_FBO: u32 = 3;
pub const MPV_RENDER_PARAM_FLIP_Y: u32 = 4;
pub const MPV_RENDER_API_TYPE_OPENGL: &str = "opengl";

// Форматы свойств из mpv/client.h
pub const MPV_FORMAT_DOUBLE: i64 = 5;

// Opaque handle'ы mpv
#[repr(C)]
pub struct MpvHandle {
    _private: [u8; 0],
}
#[repr(C)]
pub struct MpvRenderContext {
    _private: [u8; 0],
}

/// Параметры инициализации OpenGL render-контекста
#[repr(C)]
pub struct MpvOpenGLInitParams {
    pub get_proc_address: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    pub get_proc_address_ctx: *mut c_void,
    pub extra_exts: *const c_char,
}

// Сигнатуры FFI-функций libmpv
type MpvCreateFn = unsafe extern "C" fn() -> *mut MpvHandle;
type MpvInitializeFn = unsafe extern "C" fn(*mut MpvHandle) -> c_int;
type MpvTerminateDestroyFn = unsafe extern "C" fn(*mut MpvHandle);
type MpvSetOptionStringFn =
    unsafe extern "C" fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int;
type MpvSetPropertyStringFn =
    unsafe extern "C" fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int;
type MpvCommandFn = unsafe extern "C" fn(*mut MpvHandle, *const *const c_char) -> c_int;
type MpvGetPropertyFn =
    unsafe extern "C" fn(*mut MpvHandle, *const c_char, i64, *mut c_void) -> c_int;
type MpvRenderContextCreateFn = unsafe extern "C" fn(
    *mut *mut MpvRenderContext,
    *mut MpvHandle,
    *const MpvRenderParam,
) -> c_int;
type MpvRenderContextRenderFn =
    unsafe extern "C" fn(*mut MpvRenderContext, *const MpvRenderParam) -> c_int;
type MpvRenderContextFreeFn = unsafe extern "C" fn(*mut MpvRenderContext);

/// Обёртка над загруженными функциями libmpv.
/// Library хранится в поле — символы валидны пока жив MpvLib.
pub struct MpvLib {
    /// Библиотека должна жить — Symbol'и ссылаются на неё
    _lib: Library,
    mpv_create: MpvCreateFn,
    mpv_initialize: MpvInitializeFn,
    mpv_terminate_destroy: MpvTerminateDestroyFn,
    /// set_option_string — вызывается ДО mpv_initialize
    mpv_set_option_string: MpvSetOptionStringFn,
    /// set_property_string — вызывается ПОСЛЕ mpv_initialize
    mpv_set_property_string: MpvSetPropertyStringFn,
    mpv_command: MpvCommandFn,
    mpv_get_property: MpvGetPropertyFn,
    mpv_render_context_create: MpvRenderContextCreateFn,
    mpv_render_context_render: MpvRenderContextRenderFn,
    mpv_render_context_free: MpvRenderContextFreeFn,
}

// SAFETY: все функции mpv thread-safe при использовании из одного потока
unsafe impl Send for MpvLib {}
unsafe impl Sync for MpvLib {}

impl MpvLib {
    /// Загружает libmpv.so через dlopen.
    /// Пробует libmpv.so, затем libmpv.so.2.
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let lib = unsafe {
            Library::new("libmpv.so").or_else(|_| Library::new("libmpv.so.2"))?
        };
        unsafe {
            // Загружаем все символы ДО мувинга lib в struct
            let mpv_create: MpvCreateFn = load_sym(&lib, b"mpv_create\0")?;
            let mpv_initialize: MpvInitializeFn = load_sym(&lib, b"mpv_initialize\0")?;
            let mpv_terminate_destroy: MpvTerminateDestroyFn =
                load_sym(&lib, b"mpv_terminate_destroy\0")?;
            let mpv_set_option_string: MpvSetOptionStringFn =
                load_sym(&lib, b"mpv_set_option_string\0")?;
            let mpv_set_property_string: MpvSetPropertyStringFn =
                load_sym(&lib, b"mpv_set_property_string\0")?;
            let mpv_command: MpvCommandFn = load_sym(&lib, b"mpv_command\0")?;
            let mpv_get_property: MpvGetPropertyFn =
                load_sym(&lib, b"mpv_get_property\0")?;
            let mpv_render_context_create: MpvRenderContextCreateFn =
                load_sym(&lib, b"mpv_render_context_create\0")?;
            let mpv_render_context_render: MpvRenderContextRenderFn =
                load_sym(&lib, b"mpv_render_context_render\0")?;
            let mpv_render_context_free: MpvRenderContextFreeFn =
                load_sym(&lib, b"mpv_render_context_free\0")?;

            Ok(Self {
                _lib: lib,
                mpv_create,
                mpv_initialize,
                mpv_terminate_destroy,
                mpv_set_option_string,
                mpv_set_property_string,
                mpv_command,
                mpv_get_property,
                mpv_render_context_create,
                mpv_render_context_render,
                mpv_render_context_free,
            })
        }
    }

    pub unsafe fn create(&self) -> *mut MpvHandle {
        (self.mpv_create)()
    }

    pub unsafe fn initialize(&self, h: *mut MpvHandle) -> c_int {
        (self.mpv_initialize)(h)
    }

    pub unsafe fn terminate_destroy(&self, h: *mut MpvHandle) {
        (self.mpv_terminate_destroy)(h)
    }

    /// Установить опцию. Вызывать ДО initialize().
    pub unsafe fn set_option_string(
        &self,
        h: *mut MpvHandle,
        name: &str,
        val: &str,
    ) -> c_int {
        let cn = CString::new(name).unwrap();
        let cv = CString::new(val).unwrap();
        (self.mpv_set_option_string)(h, cn.as_ptr(), cv.as_ptr())
    }

    /// Установить свойство. Вызывать ПОСЛЕ initialize().
    pub unsafe fn set_property_string(
        &self,
        h: *mut MpvHandle,
        name: &str,
        val: &str,
    ) -> c_int {
        let cn = CString::new(name).unwrap();
        let cv = CString::new(val).unwrap();
        (self.mpv_set_property_string)(h, cn.as_ptr(), cv.as_ptr())
    }

    pub unsafe fn command(
        &self,
        h: *mut MpvHandle,
        args: *const *const c_char,
    ) -> c_int {
        (self.mpv_command)(h, args)
    }

    /// Получить свойство как double (time-pos, duration)
    pub unsafe fn get_property_double(
        &self,
        h: *mut MpvHandle,
        name: &str,
    ) -> Option<f64> {
        let cn = CString::new(name).unwrap();
        let mut v: f64 = 0.0;
        let r = (self.mpv_get_property)(
            h,
            cn.as_ptr(),
            MPV_FORMAT_DOUBLE,
            &mut v as *mut _ as *mut c_void,
        );
        if r == 0 {
            Some(v)
        } else {
            None
        }
    }

    pub unsafe fn render_context_create(
        &self,
        ctx: *mut *mut MpvRenderContext,
        h: *mut MpvHandle,
        p: *const MpvRenderParam,
    ) -> c_int {
        (self.mpv_render_context_create)(ctx, h, p)
    }

    pub unsafe fn render_context_render(
        &self,
        ctx: *mut MpvRenderContext,
        p: *const MpvRenderParam,
    ) -> c_int {
        (self.mpv_render_context_render)(ctx, p)
    }

    pub unsafe fn render_context_free(&self, ctx: *mut MpvRenderContext) {
        (self.mpv_render_context_free)(ctx)
    }
}

/// Загружает символ из библиотеки.
///
/// Использует Symbol::into_raw() чтобы извлечь указатель на функцию,
/// не держа borrow на Library. Это безопасно — Library хранится в
/// поле _lib struct MpvLib, поэтому символы валидны.
unsafe fn load_sym<T>(
    lib: &Library,
    name: &[u8],
) -> Result<T, Box<dyn std::error::Error>>
where
    T: Copy,
{
    let sym: Symbol<unsafe extern "C" fn()> = lib.get(name)?;
    let ptr = sym.into_raw();
    Ok(std::mem::transmute_copy(&ptr))
}
