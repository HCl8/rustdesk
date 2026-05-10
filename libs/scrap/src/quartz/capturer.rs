use std::ptr;

use block::{Block, ConcreteBlock};
use hbb_common::libc::c_void;
use std::sync::{Arc, Mutex};

use super::config::Config;
use super::display::Display;
use super::ffi::*;
use super::frame::Frame;

enum CapturerBackend {
    CGDisplayStream {
        stream: CGDisplayStreamRef,
        queue: DispatchQueue,
        stopped: Arc<Mutex<bool>>,
    },
    ScreenCaptureKit {
        manager: SCKitCaptureManagerRef,
    },
}

pub struct Capturer {
    backend: CapturerBackend,
    width: usize,
    height: usize,
    format: PixelFormat,
    display: Display,
}

impl Capturer {
    pub fn new<F: Fn(Frame) + 'static>(
        display: Display,
        width: usize,
        height: usize,
        format: PixelFormat,
        config: Config,
        handler: F,
    ) -> Result<Capturer, CGError> {
        // Try ScreenCaptureKit first (macOS 12.3+), fall back to CGDisplayStream
        if unsafe { sckit_is_available() } {
            Self::new_screencapturekit(display, width, height, format, handler)
        } else {
            Self::new_cgdisplaystream(display, width, height, format, config, handler)
        }
    }

    fn new_screencapturekit<F: Fn(Frame) + 'static>(
        display: Display,
        width: usize,
        height: usize,
        format: PixelFormat,
        handler: F,
    ) -> Result<Capturer, CGError> {
        let manager = unsafe { sckit_create() };
        if manager.is_null() {
            return Err(CGError::Failure);
        }

        // Leak the handler so it lives for the duration of capture.
        let handler_box = Box::new(handler);
        let handler_ptr = Box::into_raw(handler_box);

        let callback: SCKitFrameCallback = screencapturekit_frame_callback;
        // Store the handler pointer globally for the callback to access.
        SCREAMCAPTUREKIT_HANDLER.store(handler_ptr as *mut c_void, Ordering::Release);

        let ret = unsafe {
            sckit_start_capture(
                manager,
                display.id(),
                width as i32,
                height as i32,
                callback,
            )
        };

        if ret != 0 {
            unsafe { sckit_release(manager) };
            // Recover the handler to drop it
            let _ = unsafe { Box::from_raw(handler_ptr) };
            return Err(CGError::Failure);
        }

        Ok(Capturer {
            backend: CapturerBackend::ScreenCaptureKit { manager },
            width,
            height,
            format,
            display,
        })
    }

    fn new_cgdisplaystream<F: Fn(Frame) + 'static>(
        display: Display,
        width: usize,
        height: usize,
        format: PixelFormat,
        config: Config,
        handler: F,
    ) -> Result<Capturer, CGError> {
        let stopped = Arc::new(Mutex::new(false));
        let cloned_stopped = stopped.clone();
        let handler: FrameAvailableHandler = ConcreteBlock::new(move |status, _, surface, _| {
            use self::CGDisplayStreamFrameStatus::*;
            if status == Stopped {
                let mut lock = cloned_stopped.lock().unwrap();
                *lock = true;
                return;
            }
            if status == FrameComplete {
                handler(unsafe { Frame::new(surface) });
            }
        })
        .copy();

        let queue = unsafe {
            dispatch_queue_create(
                b"quadrupleslap.scrap\0".as_ptr() as *const i8,
                ptr::null_mut(),
            )
        };

        let stream = unsafe {
            let config = config.build();
            let stream = CGDisplayStreamCreateWithDispatchQueue(
                display.id(),
                width,
                height,
                format,
                config,
                queue,
                &*handler as *const Block<_, _> as *const c_void,
            );
            CFRelease(config);
            stream
        };

        match unsafe { CGDisplayStreamStart(stream) } {
            CGError::Success => Ok(Capturer {
                backend: CapturerBackend::CGDisplayStream {
                    stream,
                    queue,
                    stopped,
                },
                width,
                height,
                format,
                display,
            }),
            x => Err(x),
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn format(&self) -> PixelFormat {
        self.format
    }
    pub fn display(&self) -> Display {
        self.display
    }
}

impl Drop for Capturer {
    fn drop(&mut self) {
        match &self.backend {
            CapturerBackend::CGDisplayStream {
                stream,
                queue,
                stopped,
            } => {
                unsafe {
                    let _ = CGDisplayStreamStop(*stream);
                    loop {
                        if *stopped.lock().unwrap() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(30));
                    }
                    CFRelease(*stream);
                    dispatch_release(*queue);
                }
            }
            CapturerBackend::ScreenCaptureKit { manager } => {
                unsafe {
                    sckit_stop_capture(*manager);
                    sckit_release(*manager);
                }
                // Reclaim the leaked handler to free memory
                let handler_ptr = SCREAMCAPTUREKIT_HANDLER.swap(ptr::null_mut(), Ordering::AcqRel);
                if !handler_ptr.is_null() {
                    let _ = unsafe { Box::from_raw(handler_ptr as *mut Box<dyn Fn(Frame) + 'static>) };
                }
            }
        }
    }
}

// Global storage for the ScreenCaptureKit frame handler.
// The callback is invoked on a GCD queue at 30fps, so we use AtomicPtr
// for lock-free access instead of a Mutex.
use std::sync::atomic::{AtomicPtr, Ordering};
static SCREAMCAPTUREKIT_HANDLER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

extern "C" fn screencapturekit_frame_callback(
    data: *const u8,
    width: i32,
    height: i32,
    bytes_per_row: i32,
) {
    if data.is_null() || width <= 0 || height <= 0 {
        return;
    }
    let handler_ptr = SCREAMCAPTUREKIT_HANDLER.load(Ordering::Acquire);
    if !handler_ptr.is_null() {
        let handler = unsafe { &*(handler_ptr as *const Box<dyn Fn(Frame) + 'static>) };
        let data_slice =
            unsafe { std::slice::from_raw_parts(data, (bytes_per_row * height) as usize) };
        let frame = Frame::new_from_buffer(
            data_slice,
            width as usize,
            height as usize,
            bytes_per_row as usize,
        );
        handler(frame);
    }
}
