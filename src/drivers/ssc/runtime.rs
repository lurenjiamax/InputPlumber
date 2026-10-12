use std::{
    error::Error,
    ptr::null_mut,
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::drivers::ssc::bindings::{
    gpointer, FnSscSensorNewSync, FnSscSensorOpenSync, GCallback, GCancellable, GClosure, GError,
    GObject, SscDylibs, GFALSE,
};

pub type MeasurementHandlerCb = Box<dyn FnMut(f32, f32, f32) + Send>;

/// "Trampoline" for the sensor measurement callback. Bounces to the function stored in the data ptr.
pub unsafe extern "C" fn measurement_handler(
    _obj: *mut GObject,
    x: f32,
    y: f32,
    z: f32,
    data: gpointer,
) {
    let cb: &mut MeasurementHandlerCb =
        unsafe { &mut *(data as *mut MeasurementHandlerCb) };
    cb(x, y, z);
}

/// GClosureNotify that drops the boxed callback when the signal is destroyed.
pub unsafe extern "C" fn closure_destroy_handler(data: gpointer, _: *mut GClosure) {
    drop(unsafe { Box::from_raw(data as *mut MeasurementHandlerCb) });
}

/// Wrapper for libssc sensor objects (mostly just a wrapper for GObject)
pub struct SscObject {
    _ptr: *mut std::ffi::c_void,
    _runtime: Arc<SscRuntime>,
    close_fn: FnSscSensorOpenSync,
}

// SAFETY: The GLib functions we're using this with are thread safe.
unsafe impl Send for SscObject {}

impl Drop for SscObject {
    fn drop(&mut self) {
        // SAFETY: _ptr is always non-null and the library handle is kept alive by the _runtime reference.
        unsafe {
            (self.close_fn)(self.ptr(), null_mut(), null_mut());
            (self._runtime.dylibs.g_object_unref)(self.ptr() as *mut _);
        }
    }
}

impl SscObject {
    /// Create a SscObject instance. The provided pointer is required to be non-null.
    fn new(
        ptr: *mut std::ffi::c_void,
        runtime: Arc<SscRuntime>,
        close_fn: FnSscSensorOpenSync,
    ) -> Result<Self, ()> {
        if ptr.is_null() {
            Err(())
        } else {
            Ok(Self {
                _ptr: ptr,
                _runtime: runtime,
                close_fn,
            })
        }
    }

    pub unsafe fn ptr(&self) -> *mut std::ffi::c_void {
        self._ptr
    }

    pub fn set_measurement_handler<F: FnMut(f32, f32, f32) + Send + 'static>(&self, callback: F) {
        let boxed: Box<MeasurementHandlerCb> = Box::new(Box::new(callback));
        let user_data = Box::into_raw(boxed) as gpointer;

        // SAFETY: The GObject pointer is guaranteed non-null and g_signal_connect_data is kept valid by the _runtime reference.
        unsafe {
            (self._runtime.dylibs.g_signal_connect_data)(
                self._ptr as *mut _,
                c"measurement".as_ptr(),
                std::mem::transmute::<*const (), GCallback>(measurement_handler as *const ()),
                user_data,
                Some(closure_destroy_handler),
                0,
            );
        }
    }
}

/// Wrapper for GCancellable
struct Cancellable {
    _ptr: *mut GCancellable,
}

impl Cancellable {
    fn new(ptr: *mut GCancellable) -> Self {
        Self { _ptr: ptr }
    }

    pub fn ptr(&self) -> *mut GCancellable {
        self._ptr
    }

    pub fn cancel_after(timeout: Duration, runtime: Arc<SscRuntime>) -> Self {
        // SAFETY: g_cancellable_new is valid as long as runtime is valid.
        let this = unsafe { Self::new((runtime.dylibs.g_cancellable_new)()) };
        let this_ptr_u64 = this.ptr() as usize;

        std::thread::spawn(move || {
            std::thread::sleep(timeout);
            // SAFETY: These handles are valid, we keep the runtime instance alive for this thread's lifetime
            unsafe {
                (runtime.dylibs.g_cancellable_cancel)(this_ptr_u64 as *mut GCancellable);
                (runtime.dylibs.g_object_unref)(this_ptr_u64 as *mut _);
            }
        });

        this
    }
}

/// Contains the dynamic libraries and all method pointers needed for libssc to work.
/// This currently consists of: glib-2.0, gobject-2.0, gio-2.0, libssc
pub struct SscRuntime {
    pub(crate) dylibs: SscDylibs,
}

impl SscRuntime {
    pub fn load() -> Result<Arc<Self>, Box<dyn Error + Send + Sync>> {
        // GType registrations retain module function pointers beyond sensor drop.
        // Keep one loaded runtime for the entire process, including source restart.
        static RUNTIME: Mutex<Option<Arc<SscRuntime>>> = Mutex::new(None);
        let mut guard = RUNTIME.lock().map_err(|_| "SSC runtime mutex poisoned")?;
        if let Some(runtime) = guard.as_ref() {
            return Ok(runtime.clone());
        }
        let runtime = Arc::new(Self {
            // SAFETY: Gives access to a bunch of function handles. This is safe as long as we use them as intended.
            // The handles are valid as long as this SscRuntime is.
            dylibs: unsafe { SscDylibs::load()? },
        });
        *guard = Some(runtime.clone());
        Ok(runtime)
    }

    /// Converts a GLib error ptr to a basic string (or None if the error is null)
    pub fn convert_glib_error(&self, error: *mut GError) -> Option<String> {
        if error.is_null() {
            None
        } else {
            // SAFETY: The handle for g_quark_to_string is sure to be alive here
            let domain_str = unsafe {
                let domain_cstr =
                    std::ffi::CStr::from_ptr((self.dylibs.g_quark_to_string)((*error).domain));
                domain_cstr.to_string_lossy().into_owned()
            };

            let domain = unsafe { (*error).domain };
            let code = unsafe { (*error).code };

            let message = unsafe {
                std::ffi::CStr::from_ptr((*error).message)
                    .to_string_lossy()
                    .into_owned()
            };
            let result = Some(format!(
                "GLib error: domain = {} / {}, code = {}, message = {}",
                domain, domain_str, code, message
            ));
            unsafe {
                (self.dylibs.g_error_free)(error);
            }
            result
        }
    }

    /// Safe wrapper for GLib's g_main_context_iteration
    pub fn iterate_glib_main_loop(&self) {
        // SAFETY: This function may have side effects if other parts of InputPlumber start using the main context.
        // This function handle is always non-null and we don't have to pass anything to it that isn't NULL / 0.
        unsafe {
            (self.dylibs.g_main_context_iteration)(std::ptr::null_mut(), GFALSE);
        }
    }

    /// The signatures for gyroscope_(open/new)_sync and accelerometer_(open/new)_sync are the same, so we can reuse some code
    unsafe fn create_measurement_sensor(
        self: Arc<Self>,
        new_fn: FnSscSensorNewSync,
        open_fn: FnSscSensorOpenSync,
        close_fn: FnSscSensorOpenSync,
    ) -> Result<SscObject, Box<dyn Error + Send + Sync>> {
        let mut err: *mut GError = null_mut();
        let ptr = unsafe {
            let cancellable = Cancellable::cancel_after(Duration::from_secs(1), self.clone());
            (new_fn)(cancellable.ptr(), &mut err)
        };

        // Instantiate the sensor and get our GObject ptr from it
        if let Some(v) = self.convert_glib_error(err) {
            if !ptr.is_null() {
                unsafe {
                    (self.dylibs.g_object_unref)(ptr);
                }
            }
            return Err(format!("Failed to instantiate SSC sensor: {v}").into());
        }

        if ptr.is_null() {
            return Err("Failed to instantiate SSC sensor: (got a null pointer)".into());
        }

        // "Open" the sensor (make it start doing stuff)
        // note: We set the data callback later using set_measurement_handler, so this is just new sensor -> open sensor
        let opened = unsafe {
            err = std::ptr::null_mut();
            (self.dylibs.ssc_sensor_set_sample_rate)(ptr, 100.0);
            let cancellable = Cancellable::cancel_after(Duration::from_secs(4), self.clone());
            (open_fn)(ptr, cancellable.ptr(), &mut err)
        };

        if let Some(v) = self.convert_glib_error(err) {
            unsafe {
                (self.dylibs.g_object_unref)(ptr);
            }
            return Err(format!("Failed to open SSC sensor: {v}").into());
        }
        if opened == GFALSE {
            unsafe {
                (self.dylibs.g_object_unref)(ptr);
            }
            return Err("SSC open returned false without an error".into());
        }

        log::info!("SSC sensor opened: requested 100 Hz; post-libssc axes unchanged, frame/bias unvalidated");
        SscObject::new(ptr, self, close_fn)
            .map_err(|_| "Failed to create SSC measurement sensor".into())
    }

    pub fn create_gyroscope(self: Arc<Self>) -> Result<SscObject, Box<dyn Error + Send + Sync>> {
        // SAFETY: These handles will stay available as long as this SscRuntime does.
        unsafe {
            let new_fn = self.dylibs.ssc_sensor_gyroscope_new_sync;
            let open_fn = self.dylibs.ssc_sensor_gyroscope_open_sync;
            let close_fn = self.dylibs.ssc_sensor_gyroscope_close_sync;
            self.create_measurement_sensor(new_fn, open_fn, close_fn)
        }
    }

    pub fn create_accelerometer(
        self: Arc<Self>,
    ) -> Result<SscObject, Box<dyn Error + Send + Sync>> {
        // SAFETY: These handles will stay available as long as this SscRuntime does.
        unsafe {
            let new_fn = self.dylibs.ssc_sensor_accelerometer_new_sync;
            let open_fn = self.dylibs.ssc_sensor_accelerometer_open_sync;
            let close_fn = self.dylibs.ssc_sensor_accelerometer_close_sync;
            self.create_measurement_sensor(new_fn, open_fn, close_fn)
        }
    }
}
