use std::{ops, ptr, slice};

use super::ffi::*;

enum FrameSource {
    IOSurface {
        surface: IOSurfaceRef,
        inner: &'static [u8],
    },
    DirectBuffer {
        // Owned copy of BGRA pixel data from ScreenCaptureKit callback
        data: Vec<u8>,
        stride: usize,
    },
}

pub struct Frame {
    source: FrameSource,
    bgra: Vec<u8>,
    bgra_stride: usize,
}

impl Frame {
    /// Create a frame from an IOSurface (CGDisplayStream path).
    pub unsafe fn new(surface: IOSurfaceRef) -> Frame {
        CFRetain(surface);
        IOSurfaceIncrementUseCount(surface);

        IOSurfaceLock(surface, SURFACE_LOCK_READ_ONLY, ptr::null_mut());

        let inner = slice::from_raw_parts(
            IOSurfaceGetBaseAddress(surface) as *const u8,
            IOSurfaceGetAllocSize(surface),
        );

        Frame {
            source: FrameSource::IOSurface { surface, inner },
            bgra: Vec::new(),
            bgra_stride: 0,
        }
    }

    /// Create a frame from raw BGRA pixel data (ScreenCaptureKit path).
    /// Copies the data from the callback buffer so it remains valid after the callback returns.
    pub fn new_from_buffer(data: &[u8], width: usize, height: usize, bytes_per_row: usize) -> Frame {
        let mut owned_data = Vec::with_capacity(bytes_per_row * height);
        let copy_len = bytes_per_row * height;
        let src = &data[..copy_len.min(data.len())];
        owned_data.extend_from_slice(src);

        Frame {
            source: FrameSource::DirectBuffer {
                data: owned_data,
                stride: bytes_per_row,
            },
            bgra: Vec::new(),
            bgra_stride: 0,
        }
    }

    #[inline]
    pub fn inner(&self) -> &[u8] {
        match &self.source {
            FrameSource::IOSurface { inner, .. } => inner,
            FrameSource::DirectBuffer { data, .. } => data,
        }
    }

    pub fn stride(&self) -> usize {
        self.bgra_stride
    }

    pub fn surface_to_bgra<'a>(&'a mut self, h: usize) {
        match &self.source {
            FrameSource::IOSurface { surface, .. } => {
                unsafe {
                    let plane0 = IOSurfaceGetBaseAddressOfPlane(*surface, 0);
                    self.bgra_stride = IOSurfaceGetBytesPerRowOfPlane(*surface, 0);
                    self.bgra.resize(self.bgra_stride * h, 0);
                    std::ptr::copy_nonoverlapping(
                        plane0 as _,
                        self.bgra.as_mut_ptr(),
                        self.bgra_stride * h,
                    );
                }
            }
            FrameSource::DirectBuffer { data, stride } => {
                // Data is already in BGRA format, just copy row by row
                self.bgra_stride = *stride;
                let total = self.bgra_stride * h;
                self.bgra.resize(total, 0);
                let copy_len = total.min(data.len());
                self.bgra[..copy_len].copy_from_slice(&data[..copy_len]);
            }
        }
    }
}

impl ops::Deref for Frame {
    type Target = [u8];
    fn deref<'a>(&'a self) -> &'a [u8] {
        &self.bgra
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        if let FrameSource::IOSurface { surface, .. } = &self.source {
            unsafe {
                IOSurfaceUnlock(*surface, SURFACE_LOCK_READ_ONLY, ptr::null_mut());
                IOSurfaceDecrementUseCount(*surface);
                CFRelease(*surface);
            }
        }
        // DirectBuffer: Vec<u8> drops automatically
    }
}
