#pragma once
#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*SCKitFrameCallback)(const uint8_t *data, int width, int height, int bytes_per_row);
typedef void *SCKitCaptureManagerRef;

// Returns true if ScreenCaptureKit is available (macOS 12.3+).
bool sckit_is_available(void);

// Create a capture manager instance. Returns NULL on failure.
SCKitCaptureManagerRef sckit_create(void);

// Release a capture manager instance.
void sckit_release(SCKitCaptureManagerRef manager);

// Start capturing the specified display. The callback is invoked on a private dispatch queue
// for each new frame. The callback receives BGRA pixel data, dimensions, and bytes-per-row.
// The data pointer is only valid during the callback invocation.
// Returns 0 on success, negative on error.
int sckit_start_capture(SCKitCaptureManagerRef manager, uint32_t displayID,
                        int width, int height, SCKitFrameCallback callback);

// Stop capturing. Returns 0 on success, negative on error.
int sckit_stop_capture(SCKitCaptureManagerRef manager);

#ifdef __cplusplus
}
#endif
