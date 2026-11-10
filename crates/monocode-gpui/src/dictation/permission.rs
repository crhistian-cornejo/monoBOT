//! Asks macOS for the microphone before dictation opens it (43308a3b). The
//! bundle's `NSMicrophoneUsageDescription` and the audio-input entitlement
//! let it ask at all; without an answer of yes, CoreAudio would record
//! silence, so the composer says why instead.

/// Ok once the user allowed the microphone; asks the first time.
#[cfg(target_os = "macos")]
pub(super) fn microphone() -> Result<(), String> {
    use block2::RcBlock;
    use objc2::{msg_send, runtime::AnyClass, runtime::Bool};
    use objc2_foundation::NSString;

    #[link(name = "AVFoundation", kind = "framework")]
    unsafe extern "C" {
        static AVMediaTypeAudio: &'static NSString;
    }
    // AVAuthorizationStatus.
    const NOT_DETERMINED: isize = 0;
    const AUTHORIZED: isize = 3;
    const DENIED: &str = "Microphone access is off for monoBOT. Turn it on in System Settings → Privacy & Security → Microphone.";

    let Some(device) = AnyClass::get(c"AVCaptureDevice") else {
        return Err("Microphone: AVFoundation is not available".into());
    };
    // SAFETY: AVMediaTypeAudio is AVFoundation's constant NSString.
    let media = unsafe { AVMediaTypeAudio };
    // SAFETY: +[AVCaptureDevice authorizationStatusForMediaType:] takes an
    // AVMediaType and returns an NSInteger.
    let status: isize = unsafe { msg_send![device, authorizationStatusForMediaType: media] };
    match status {
        AUTHORIZED => Ok(()),
        NOT_DETERMINED => {
            let (answer, answered) = std::sync::mpsc::channel();
            let handler = RcBlock::new(move |granted: Bool| {
                let _ = answer.send(granted.as_bool());
            });
            // SAFETY: the completion handler is a `void (^)(BOOL)` block,
            // which AVFoundation copies and calls once on a queue of its own.
            unsafe {
                let _: () = msg_send![
                    device,
                    requestAccessForMediaType: media,
                    completionHandler: &*handler
                ];
            }
            match answered.recv() {
                Ok(true) => Ok(()),
                _ => Err(DENIED.into()),
            }
        }
        _ => Err(DENIED.into()),
    }
}

/// Windows asks nothing up front: its privacy switch fails the open, and
/// cpal's error says so.
#[cfg(not(target_os = "macos"))]
pub(super) fn microphone() -> Result<(), String> {
    Ok(())
}
