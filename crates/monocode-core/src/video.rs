//! Video files as moving backgrounds. The system decodes them (Media
//! Foundation on Windows, with its own hardware decoders and scaler) into
//! small frames: a background shows at a fraction of its opacity behind the
//! chat, and its effects are pictures of few pixels, so a 4K clip is decoded
//! straight to the size it is drawn at rather than to 4K and shrunk after.
//! Nothing here keeps frames: each one goes to the caller as it comes, so a
//! long video costs one frame of memory, not the whole clip.
use image::RgbaImage;
use std::{path::Path, time::Duration};

/// The files a background may be a video of.
pub const EXTENSIONS: [&str; 7] = ["mp4", "m4v", "mov", "webm", "mkv", "avi", "wmv"];

/// Whether `path` names a video, by its extension.
pub fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
}

/// One decoded frame and when it shows, counted from the first frame of the
/// first time through: it keeps growing across loops.
pub struct Frame {
    pub picture: RgbaImage,
    pub at: Duration,
}

/// Decodes `path` from its start, over and over, into frames that fit
/// `max_side`, at most `fps` a second, and hands each to `on_frame` until it
/// returns false. Blocks for as long as it plays, so it runs on a thread of
/// its own; it does not pace itself, the caller does by when it takes frames.
pub fn play(
    path: &Path,
    max_side: u32,
    fps: u32,
    on_frame: &mut dyn FnMut(Frame) -> bool,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::play(path, max_side, fps, on_frame)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, max_side, fps, on_frame);
        Err("Video backgrounds play on Windows for now.".into())
    }
}

/// The size a `width`×`height` video is decoded at to fit `max_side`: even
/// sides, as video scalers want, and never larger than the video.
pub fn fitted(width: u32, height: u32, max_side: u32) -> (u32, u32) {
    let scale = (max_side as f64 / width.max(height).max(1) as f64).min(1.);
    let side = |length: u32| (((length as f64 * scale).round() as u32) & !1).max(2);
    (side(width), side(height))
}

#[cfg(windows)]
mod windows {
    use super::{fitted, Frame};
    use image::RgbaImage;
    use std::{path::Path, time::Duration};
    use windows::{
        core::{GUID, HSTRING},
        Win32::{
            Media::MediaFoundation::*,
            System::Com::{
                CoInitializeEx, CoUninitialize, StructuredStorage::PROPVARIANT,
                COINIT_MULTITHREADED,
            },
        },
    };

    fn text(error: windows::core::Error) -> String {
        format!("This video could not be played: {}", error.message())
    }

    pub(super) fn play(
        path: &Path,
        max_side: u32,
        fps: u32,
        on_frame: &mut dyn FnMut(Frame) -> bool,
    ) -> Result<(), String> {
        // SAFETY: this thread joins COM and Media Foundation for the call and
        // leaves both before returning; every interface is released before.
        unsafe {
            let com = CoInitializeEx(None, COINIT_MULTITHREADED);
            let result = match MFStartup(MF_VERSION, MFSTARTUP_LITE) {
                Ok(()) => {
                    let result = decode(path, max_side, fps, on_frame);
                    let _ = MFShutdown();
                    result
                }
                Err(error) => Err(text(error)),
            };
            if com.is_ok() {
                CoUninitialize();
            }
            result
        }
    }

    unsafe fn decode(
        path: &Path,
        max_side: u32,
        fps: u32,
        on_frame: &mut dyn FnMut(Frame) -> bool,
    ) -> Result<(), String> {
        unsafe {
            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 2).map_err(text)?;
            let attributes = attributes.ok_or("Media Foundation is not available.")?;
            // Color conversion and scaling by the reader itself, on the
            // hardware decoder where there is one.
            attributes
                .SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)
                .map_err(text)?;
            attributes
                .SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)
                .map_err(text)?;
            let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), &attributes)
                .map_err(text)?;
            let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
            // Only the picture: no audio is decoded for nothing.
            reader
                .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
                .map_err(text)?;
            reader.SetStreamSelection(stream, true).map_err(text)?;
            let native = reader.GetNativeMediaType(stream, 0).map_err(text)?;
            let size = native.GetUINT64(&MF_MT_FRAME_SIZE).map_err(text)?;
            let (width, height) = fitted((size >> 32) as u32, size as u32, max_side);
            let wanted = MFCreateMediaType().map_err(text)?;
            wanted
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(text)?;
            wanted
                .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
                .map_err(text)?;
            wanted
                .SetUINT64(&MF_MT_FRAME_SIZE, ((width as u64) << 32) | height as u64)
                .map_err(text)?;
            reader
                .SetCurrentMediaType(stream, None, &wanted)
                .map_err(text)?;
            let current = reader.GetCurrentMediaType(stream).map_err(text)?;
            let size = current.GetUINT64(&MF_MT_FRAME_SIZE).map_err(text)?;
            let (width, height) = ((size >> 32) as u32, size as u32);
            // Rows go top to bottom unless the stride says otherwise.
            let stride = current
                .GetUINT32(&MF_MT_DEFAULT_STRIDE)
                .map(|stride| stride as i32)
                .unwrap_or(width as i32 * 4);
            let row = width as usize * 4;
            let every = Duration::from_secs(1) / fps.max(1);
            // Where this time through starts, and the last frame seen in it.
            let mut base = Duration::ZERO;
            let mut last = None::<Duration>;
            let mut next = Duration::ZERO;
            loop {
                let mut flags = 0u32;
                let mut stamp = 0i64;
                let mut sample = None;
                reader
                    .ReadSample(
                        stream,
                        0,
                        None,
                        Some(&mut flags),
                        Some(&mut stamp),
                        Some(&mut sample),
                    )
                    .map_err(text)?;
                if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    // Back to the start, one frame after the last.
                    let Some(end) = last.take() else {
                        return Err("This video has no frames to show.".into());
                    };
                    base = end + every;
                    reader
                        .SetCurrentPosition(&GUID::zeroed(), &PROPVARIANT::from(0i64))
                        .map_err(text)?;
                    continue;
                }
                let Some(sample) = sample else {
                    continue;
                };
                let at = base + Duration::from_nanos(stamp.max(0) as u64 * 100);
                last = Some(at);
                if at < next {
                    continue;
                }
                next = at + every;
                let buffer = sample.ConvertToContiguousBuffer().map_err(text)?;
                let mut data = std::ptr::null_mut();
                let mut length = 0u32;
                buffer
                    .Lock(&mut data, None, Some(&mut length))
                    .map_err(text)?;
                let bytes = std::slice::from_raw_parts(data, length as usize);
                let mut picture = RgbaImage::new(width, height);
                let pitch = stride.unsigned_abs() as usize;
                for (y, out) in picture.chunks_exact_mut(row).enumerate() {
                    let source_row = if stride < 0 {
                        height as usize - 1 - y
                    } else {
                        y
                    };
                    let Some(source) = bytes.get(source_row * pitch..source_row * pitch + row)
                    else {
                        break;
                    };
                    // RGB32 is blue, green, red and an unused byte.
                    for (to, from) in out
                        .as_chunks_mut::<4>()
                        .0
                        .iter_mut()
                        .zip(source.as_chunks::<4>().0)
                    {
                        *to = [from[2], from[1], from[0], 255];
                    }
                }
                let _ = buffer.Unlock();
                if !on_frame(Frame { picture, at }) {
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_video_is_told_by_its_extension_and_fitted_to_even_sides() {
        assert!(is_video(Path::new("C:/clips/Ocean.MP4")));
        assert!(is_video(Path::new("/clips/loop.webm")));
        assert!(!is_video(Path::new("/clips/loop.gif")));
        assert_eq!(fitted(3840, 2160, 960), (960, 540));
        assert_eq!(fitted(1080, 1920, 960), (540, 960));
        // Never larger than the video.
        assert_eq!(fitted(640, 360, 960), (640, 360));
        assert_eq!(fitted(1001, 501, 960), (960, 480));
    }

    /// Plays a real clip: `MONOBOT_TEST_VIDEO=<path> cargo test … -- --ignored`.
    #[test]
    #[ignore]
    fn a_real_clip_decodes_scaled_and_loops() {
        let Some(path) = std::env::var_os("MONOBOT_TEST_VIDEO") else {
            return;
        };
        let mut frames: Vec<(u32, u32, Duration)> = Vec::new();
        let out = std::env::var_os("MONOBOT_TEST_VIDEO_FRAME");
        play(Path::new(&path), 960, 24, &mut |frame| {
            // To look at: the first frame as decoded.
            if let (true, Some(out)) = (frames.is_empty(), &out) {
                frame.picture.save(out).unwrap();
            }
            frames.push((frame.picture.width(), frame.picture.height(), frame.at));
            frames.len() < 120
        })
        .unwrap();
        assert_eq!(frames.len(), 120);
        assert!(frames.iter().all(|(w, h, _)| *w <= 960 && *h <= 960));
        // Time only moves forward, across loops too, and at most 24 a second.
        for pair in frames.windows(2) {
            assert!(pair[1].2 > pair[0].2, "{:?}", (pair[0].2, pair[1].2));
            assert!(pair[1].2 - pair[0].2 >= Duration::from_millis(40));
        }
    }
}
