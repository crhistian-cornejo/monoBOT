//! Which microphone to open and where the voice goes, as IVY tells them
//! apart (`IVY/crates/voice/src/capture.rs`).
//!
//! Opening a Bluetooth headset's microphone drops the headset to call quality
//! (from A2DP to HFP), music and all, for as long as the microphone is open,
//! and clips the start of whatever plays next. With a headset on, the
//! computer's own microphone hears just as well.
use cpal::traits::{DeviceTrait, HostTrait};

/// What CoreAudio calls the Mac's own microphone.
const MAC_OWN_MICROPHONE: &str = "BuiltInMicrophoneDevice";

/// The microphone to dictate with: the default one, unless that is a
/// Bluetooth headset's and the computer has one of its own.
pub fn microphone() -> Option<cpal::Device> {
    let host = cpal::default_host();
    let default = host.default_input_device()?;
    if !on_headset(&default) {
        return Some(default);
    }
    let others: Vec<cpal::Device> = host
        .input_devices()
        .map(|all| all.filter(|device| !on_headset(device)).collect())
        .unwrap_or_default();
    let builtin = |device: &&cpal::Device| {
        device
            .id()
            .is_ok_and(|id| id.to_string().contains(MAC_OWN_MICROPHONE))
            || device
                .description()
                .is_ok_and(|about| about.interface_type() == cpal::InterfaceType::BuiltIn)
    };
    // Windows does not always say which one is its own: then the first that
    // is not a virtual device some program installed.
    let real = |device: &&cpal::Device| {
        cfg!(windows)
            && device
                .description()
                .is_ok_and(|about| about.interface_type() != cpal::InterfaceType::Virtual)
    };
    let own = others
        .iter()
        .find(builtin)
        .or_else(|| others.iter().find(real))
        .cloned();
    Some(own.unwrap_or(default))
}

/// Whether what is played goes straight into the user's ears, so no
/// microphone hears it back: headphones, or a Bluetooth device with a
/// microphone of its own (a Bluetooth speaker has none and fills the room).
pub fn in_ears() -> bool {
    let host = cpal::default_host();
    let Some(output) = host.default_output_device() else {
        return false;
    };
    let id = output.id().map(|id| id.to_string()).unwrap_or_default();
    if id.contains("BuiltInHeadphone") {
        return true;
    }
    if output.description().is_ok_and(|about| {
        matches!(
            about.device_type(),
            cpal::DeviceType::Headphones | cpal::DeviceType::Headset
        )
    }) {
        return true;
    }
    let Some(address) = bluetooth_address(&id, ":output") else {
        return false;
    };
    host.input_devices().is_ok_and(|mut all| {
        all.any(|device| {
            device.id().is_ok_and(|id| {
                bluetooth_address(&id.to_string(), ":input").as_deref() == Some(&address)
            })
        })
    })
}

fn on_headset(device: &cpal::Device) -> bool {
    device.description().is_ok_and(|about| {
        about.interface_type() == cpal::InterfaceType::Bluetooth || headset_name(about.name())
    }) || device
        .id()
        .is_ok_and(|id| bluetooth_address(&id.to_string(), ":input").is_some())
}

/// How Windows names a headset's microphone in its call profile.
fn headset_name(name: &str) -> bool {
    let name = name.to_lowercase();
    ["hands-free", "manos libres", "bluetooth"]
        .iter()
        .any(|word| name.contains(word))
}

/// The Bluetooth address in a CoreAudio device id that ends in `direction`
/// («aa-bb-cc-dd-ee-ff:input»), when it is one; cpal does not say how a
/// Mac's device is connected.
fn bluetooth_address(id: &str, direction: &str) -> Option<String> {
    let address = id.strip_suffix(direction)?;
    let address = address.rsplit(':').next().unwrap_or(address);
    let pairs: Vec<&str> = address.split('-').collect();
    (pairs.len() == 6
        && pairs
            .iter()
            .all(|pair| pair.len() == 2 && pair.chars().all(|c| c.is_ascii_hexdigit())))
    .then(|| address.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_coreaudio_bluetooth_id_names_its_address() {
        assert_eq!(
            bluetooth_address("coreaudio:A0-B1-C2-D3-E4-F5:input", ":input").as_deref(),
            Some("a0-b1-c2-d3-e4-f5")
        );
        assert_eq!(
            bluetooth_address("A0-B1-C2-D3-E4-F5:output", ":output").as_deref(),
            Some("a0-b1-c2-d3-e4-f5")
        );
        // The wrong direction, the Mac's own devices and short ids are none.
        assert_eq!(
            bluetooth_address("A0-B1-C2-D3-E4-F5:output", ":input"),
            None
        );
        assert_eq!(
            bluetooth_address("BuiltInMicrophoneDevice:input", ":input"),
            None
        );
        assert_eq!(bluetooth_address("A0-B1-C2-D3-E4:input", ":input"), None);
        assert_eq!(bluetooth_address("A0-B1-C2-D3-E4-GG:input", ":input"), None);
    }

    #[test]
    fn a_hands_free_name_is_a_headset() {
        assert!(headset_name("Headset (WH-1000XM5 Hands-Free AG Audio)"));
        assert!(headset_name("Auriculares (Manos libres)"));
        assert!(!headset_name("MacBook Pro Microphone"));
    }

    /// What this computer has right now: `cargo test -p monobot-dictation --
    /// --ignored --nocapture devices`.
    #[test]
    #[ignore]
    fn devices_on_this_computer() {
        let mic = microphone().map(|device| device.id().map(|id| id.to_string()));
        eprintln!("microphone: {mic:?}, voice in ears: {}", in_ears());
    }
}
