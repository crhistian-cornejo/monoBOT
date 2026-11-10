//! How loud the sound cues play.
//!
//! Port of the volume part of `src/features/settings/model/sounds.ts`
//! (`SOUNDS_VOLUME_DEFAULT`, `SOUNDS_BOOST`, `loadSoundsVolume`,
//! `saveSoundsVolume`) and the "Sound volume" slider of the settings. The cues
//! themselves (and the on/off switch) live in the app crate's `sound.rs`.
//!
//! The user sets a percentage (0 to 100) of the loudest monoBOT plays; the
//! system volume still scales it, as it does any app's sound. The cues were
//! quiet on laptop speakers and headphones, since the engine's limiter keeps
//! peaks near -8 dBFS, so the output is lifted by [`BOOST`] after the limiter:
//! about +5 dB, peaks still under full scale. The gain the player applies is
//! [`output_gain`]; for players that cannot scale (such as `afplay` fed a
//! prebuilt file) [`scale_wav`] applies it to 16-bit PCM WAV data.
//!
//! Persistence: the store setting [`SETTING_KEY`] (`monocode.soundsVolume`, the
//! same key Tauri keeps in `localStorage`), an integer in percent as text.
//! [`load`] also reads Tauri's value while the native app has none.

use crate::desktop_preferences::DesktopPreferences;
use crate::store::Store;

/// Store setting (and Tauri `localStorage` key) of the volume.
pub const SETTING_KEY: &str = "monocode.soundsVolume";

/// The volume when nobody chose: the loudest monoBOT plays.
pub const VOLUME_DEFAULT: u8 = 100;

/// Gain applied after the engine's limiter (about +5 dB).
pub const BOOST: f32 = 1.8;

/// Past the longest cue's tail (arrival, ~1.75 s), with room to spare: close the
/// audio output after this long without a cue.
pub const SUSPEND_MS: u64 = 4_000;

/// A volume as the settings slider holds it: rounded, within 0 to 100.
pub fn clamp(value: f64) -> u8 {
    if value.is_finite() {
        value.round().clamp(0.0, 100.0) as u8
    } else {
        VOLUME_DEFAULT
    }
}

/// Parse the stored text; anything that is not a finite number is the default.
pub fn parse(raw: Option<&str>) -> u8 {
    raw.and_then(|raw| raw.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .map_or(VOLUME_DEFAULT, clamp)
}

/// The engine volume, 0.0 to 1.0, for a percentage.
pub fn engine_volume(volume: u8) -> f32 {
    f32::from(volume.min(100)) / 100.0
}

/// The gain to apply to a cue's samples at `volume` percent: the engine volume
/// times the lift. At 100 it is [`BOOST`]; at 0 it is silence.
pub fn output_gain(volume: u8) -> f32 {
    engine_volume(volume) * BOOST
}

/// The saved volume: the native one, else the one the Tauri app saved, else
/// [`VOLUME_DEFAULT`].
pub fn load(store: &Store, tauri: Option<&DesktopPreferences>) -> u8 {
    let raw = store.setting(SETTING_KEY).ok().flatten().or_else(|| {
        tauri
            .and_then(|prefs| prefs.get(SETTING_KEY))
            .map(String::from)
    });
    parse(raw.as_deref())
}

/// Save the volume (clamped); returns what was saved. The UI plays a cue once
/// the slider rests so the new level can be heard.
pub fn save(store: &Store, volume: f64) -> Result<u8, String> {
    let volume = clamp(volume);
    store.save_setting(SETTING_KEY, &volume.to_string())?;
    Ok(volume)
}

/// Scale the 16-bit PCM samples of a canonical WAV (44-byte header, as the app
/// writes its cues) by `gain`, clamping at full scale. Returns `None` when the
/// data is not such a file.
pub fn scale_wav(wav: &[u8], gain: f32) -> Option<Vec<u8>> {
    if wav.len() < 44 || &wav[..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let mut out = wav.to_vec();
    for sample in out[44..].as_chunks_mut::<2>().0 {
        let value = f32::from(i16::from_le_bytes(*sample)) * gain;
        let scaled = value
            .round()
            .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
        *sample = scaled.to_le_bytes();
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_at_full_volume_until_the_user_turns_it_down() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        assert_eq!(load(&store, None), 100);
        assert_eq!(save(&store, 40.0).unwrap(), 40);
        assert_eq!(load(&store, None), 40);
        assert_eq!(engine_volume(load(&store, None)), 0.4);
        assert_eq!(save(&store, 140.0).unwrap(), 100);
        assert_eq!(load(&store, None), 100);
        store.save_setting(SETTING_KEY, "loud").unwrap();
        assert_eq!(load(&store, None), VOLUME_DEFAULT);
    }

    #[test]
    fn clamps_and_rounds() {
        assert_eq!(clamp(-5.0), 0);
        assert_eq!(clamp(33.6), 34);
        assert_eq!(clamp(f64::NAN), VOLUME_DEFAULT);
        assert_eq!(parse(Some(" 55 ")), 55);
        assert_eq!(parse(Some("-3")), 0);
        assert_eq!(parse(None), VOLUME_DEFAULT);
    }

    #[test]
    fn lifts_the_output_by_about_five_decibels() {
        assert_eq!(output_gain(100), BOOST);
        assert_eq!(output_gain(0), 0.0);
        assert!((output_gain(50) - 0.9).abs() < 1e-6);
        let db = 20.0 * BOOST.log10();
        assert!((db - 5.1).abs() < 0.1, "{db}");
    }

    #[test]
    fn scales_wav_samples_and_never_wraps() {
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&[0; 4]);
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(&[0; 32]);
        for sample in [1000i16, -1000, 30000, -30000] {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        let scaled = scale_wav(&wav, BOOST).unwrap();
        let samples: Vec<i16> = scaled[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c))
            .collect();
        assert_eq!(samples, [1800, -1800, i16::MAX, i16::MIN]);
        assert_eq!(&scaled[..44], &wav[..44]);
        assert!(scale_wav(b"not a wav", 1.0).is_none());
    }
}
