//! Explicit opt-in candidate for exact predecessor compression using the pinned Node fork.

unsafe extern "C" {
    fn weather_node_gzip(
        input: *const u8,
        input_size: u32,
        output: *mut u8,
        output_size: u32,
        written: *mut u32,
    ) -> std::ffi::c_int;
}

/// Encode one level-nine gzip member with the original Node fork and fixed default header.
pub fn native_node_gzip_candidate(bytes: &[u8]) -> Option<Vec<u8>> {
    let input_size = u32::try_from(bytes.len()).ok()?;
    // reserve the fork's conservative fixed-block bound plus gzip framing
    let limit = bytes
        .len()
        .checked_add(bytes.len() >> 3)?
        .checked_add(bytes.len() >> 8)?
        .checked_add(bytes.len() >> 9)?
        .checked_add(22)?;
    let output_size = u32::try_from(limit).ok()?;
    let mut output = Vec::new();
    output.try_reserve_exact(limit).ok()?;
    let mut written = 0;
    // safety: the caller buffer is reserved for the exact writable capacity and the
    // source is immutable for this synchronous call; initialized bytes are committed only on success
    let status = unsafe {
        weather_node_gzip(
            bytes.as_ptr(),
            input_size,
            output.as_mut_ptr(),
            output_size,
            &mut written,
        )
    };
    // retain only a complete admitted member within the allocated capacity
    if status != 1 || written > output_size {
        return None;
    }
    // safety: the C compressor reports the exact initialized contiguous output length
    unsafe { output.set_len(written as usize) };
    Some(output)
}

use std::io::{self, Write};
use std::ptr::NonNull;

unsafe extern "C" {
    fn weather_node_gzip_stream_new() -> *mut std::ffi::c_void;
    fn weather_node_gzip_stream_step(
        state: *mut std::ffi::c_void,
        input: *const u8,
        input_size: u32,
        output: *mut u8,
        output_size: u32,
        finish: std::ffi::c_int,
        consumed: *mut u32,
        written: *mut u32,
    ) -> std::ffi::c_int;
    fn weather_node_gzip_stream_free(state: *mut std::ffi::c_void);
}

/// The same deterministic candidate compressor with bounded streaming storage.
pub struct NativeNodeGzipWriter<W: Write> {
    state: NonNull<std::ffi::c_void>,
    sink: Option<W>,
    buffer: Box<[u8; 32 * 1024]>,
    failed: bool,
}

impl<W: Write> NativeNodeGzipWriter<W> {
    /// Construct the exact predecessor gzip stream with one reusable output buffer.
    pub fn new(sink: W) -> io::Result<Self> {
        // safety: the C owner allocates a private initialized zlib stream
        let state = NonNull::new(unsafe { weather_node_gzip_stream_new() })
            .ok_or_else(|| io::Error::other("native gzip initialization failed"))?;
        Ok(Self {
            state,
            sink: Some(sink),
            buffer: Box::new([0; 32 * 1024]),
            failed: false,
        })
    }

    /// Drive one bounded input segment through the same zlib state.
    fn step(&mut self, input: &[u8], finish: bool) -> io::Result<(usize, bool)> {
        let mut consumed = 0;
        let mut written = 0;
        // safety: the state is privately owned and both input and output regions remain valid
        let status = unsafe {
            weather_node_gzip_stream_step(
                self.state.as_ptr(),
                input.as_ptr(),
                input.len() as u32,
                self.buffer.as_mut_ptr(),
                self.buffer.len() as u32,
                i32::from(finish),
                &mut consumed,
                &mut written,
            )
        };
        // retain only actual progress from one admitted native step
        if !(1..=2).contains(&status)
            || consumed as usize > input.len()
            || written as usize > self.buffer.len()
            || (status == 1 && consumed == 0 && written == 0)
        {
            self.failed = true;
            return Err(io::Error::other("native gzip stream failed"));
        }
        // abort the stream if its destination cannot retain the emitted chunk
        if let Err(error) = self
            .sink
            .as_mut()
            .expect("owned sink")
            .write_all(&self.buffer[..written as usize])
        {
            self.failed = true;
            return Err(error);
        }
        Ok((consumed as usize, status == 2))
    }

    /// Emit the original trailer and return the completed sink.
    pub fn finish(mut self) -> io::Result<W> {
        if self.failed {
            return Err(io::Error::other("native gzip stream failed"));
        }
        // drain only the remaining native stream and its exact trailer
        while !self.step(&[], true)?.1 {}
        Ok(self.sink.take().expect("owned sink"))
    }
}

impl<W: Write> Write for NativeNodeGzipWriter<W> {
    /// Consume one bounded segment without flushing or creating extra gzip members.
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if self.failed {
            return Err(io::Error::other("native gzip stream failed"));
        }
        if input.is_empty() {
            return Ok(0);
        }
        let part = &input[..input.len().min(u32::MAX as usize)];
        // zlib may first drain pending bytes into the fixed output buffer
        loop {
            let (consumed, finished) = self.step(part, false)?;
            if finished {
                self.failed = true;
                return Err(io::Error::other("native gzip stream ended early"));
            }
            if consumed != 0 {
                return Ok(consumed);
            }
        }
    }

    /// Flush only the downstream sink without changing deterministic zlib framing.
    fn flush(&mut self) -> io::Result<()> {
        self.sink.as_mut().expect("owned sink").flush()
    }
}

impl<W: Write> Drop for NativeNodeGzipWriter<W> {
    /// Release the native state even after an interrupted sink.
    fn drop(&mut self) {
        // safety: the exact unique native stream is released once
        unsafe { weather_node_gzip_stream_free(self.state.as_ptr()) };
    }
}

#[cfg(test)]
#[path = "native_node_gzip_candidate_test.rs"]
mod tests;
