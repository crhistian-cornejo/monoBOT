//! Update signing for MonoBOT releases, without the Tauri CLI.
//!
//! Keys and signatures use the format the app's update client verifies
//! (`app_update`): base64 of the minisign key or signature box text.
//!
//! ```sh
//! # One time: create the key pair. The password comes from the environment.
//! MONOBOT_UPDATER_PRIVATE_KEY_PASSWORD=… sign_native_update generate <dir>
//! # Release: write <file>.sig next to each file.
//! MONOBOT_UPDATER_PRIVATE_KEY=… MONOBOT_UPDATER_PRIVATE_KEY_PASSWORD=… sign_native_update sign <file>…
//! ```
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign::{KeyPair, SecretKeyBox};
use std::{env, fs, path::Path, time::SystemTime};

const KEY_COMMENT: &str = "MonoBOT update signing key";
const SIGNATURE_COMMENT: &str = "signature from MonoBOT update signing key";

fn password() -> String {
    env::var("MONOBOT_UPDATER_PRIVATE_KEY_PASSWORD").unwrap_or_default()
}

fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    if path.exists() {
        return Err(format!(
            "{} already exists; refusing to overwrite a key",
            path.display()
        ));
    }
    fs::write(path, contents).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn generate(dir: &Path) -> Result<(), String> {
    let pair = KeyPair::generate_encrypted_keypair(Some(password())).map_err(|e| e.to_string())?;
    let secret = pair
        .sk
        .to_box(Some(KEY_COMMENT))
        .map_err(|e| e.to_string())?;
    let public = pair.pk.to_box().map_err(|e| e.to_string())?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    write_private(
        &dir.join("monobot-updater.key"),
        &STANDARD.encode(secret.into_string()),
    )?;
    fs::write(
        dir.join("monobot-updater.key.pub"),
        STANDARD.encode(public.into_string()),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "Wrote monobot-updater.key and monobot-updater.key.pub in {}",
        dir.display()
    );
    Ok(())
}

fn sign(files: &[String]) -> Result<(), String> {
    if files.is_empty() {
        return Err("Expected at least one file to sign".into());
    }
    let encoded = env::var("MONOBOT_UPDATER_PRIVATE_KEY")
        .map_err(|_| "Missing MONOBOT_UPDATER_PRIVATE_KEY")?;
    let text = String::from_utf8(
        STANDARD
            .decode(encoded.trim())
            .map_err(|_| "Invalid private key encoding")?,
    )
    .map_err(|_| "Invalid private key encoding")?;
    let secret = SecretKeyBox::from_string(&text)
        .and_then(|key| key.into_secret_key(Some(password())))
        .map_err(|_| "Could not open the private key (wrong key or password)")?;
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    for file in files {
        let path = Path::new(file);
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("Invalid file name")?;
        let data = fs::File::open(path).map_err(|e| format!("{file}: {e}"))?;
        let trusted = format!("timestamp:{timestamp}\tfile:{name}");
        let signature =
            minisign::sign(None, &secret, data, Some(&trusted), Some(SIGNATURE_COMMENT))
                .map_err(|e| format!("{file}: {e}"))?;
        fs::write(
            format!("{file}.sig"),
            STANDARD.encode(signature.into_string()),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("generate") if args.len() == 2 => generate(Path::new(&args[1])),
        Some("sign") => sign(&args[1..]),
        _ => Err("Usage: sign_native_update generate <dir> | sign <file>…".into()),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
