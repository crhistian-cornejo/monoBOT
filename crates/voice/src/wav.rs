//! Speech as the webview plays it: a 16-bit mono WAV the page decodes with
//! Web Audio, so playback, queueing and interrupting stay in the call's UI.

pub fn encode(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let clamped = if sample.is_finite() {
            sample.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        out.extend_from_slice(&((clamped * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn writes_a_pcm_header_and_clamped_samples() {
        let wav = encode(&[0.0, 1.0, -2.0, f32::NAN], 24_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 24_000);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 8);
        assert_eq!(wav.len(), 44 + 8);
        let sample = |i: usize| i16::from_le_bytes(wav[44 + i * 2..46 + i * 2].try_into().unwrap());
        assert_eq!(sample(0), 0);
        assert_eq!(sample(1), i16::MAX);
        assert_eq!(sample(2), -i16::MAX);
        assert_eq!(sample(3), 0);
    }
}
