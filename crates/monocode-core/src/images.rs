//! A Mono's `images.generate` action: Gemini's Nano Banana, reached through
//! an Antigravity turn.
//!
//! Ports `src/features/images/model/generateImages.ts`, the `images.generate`
//! branch of `src/features/agent-app/model/agentApp.ts`, the job record of
//! `src/features/sessions/model/session.ts` (`ImageJobMeta`) and the picture
//! collection of `src-tauri/src/fs.rs` (`collect_antigravity_images`).
//!
//! The Tauri app never calls the Gemini API and holds no key. It runs one
//! throwaway Antigravity (`agy`) turn that is told to use its
//! `generate_image` tool, auto-approves that tool and refuses everything
//! else, then copies the pictures Gemini wrote under
//! `~/.gemini/antigravity-cli/brain` into the app's `generated-images`
//! folder. Sign-in is Antigravity's own, so there is nothing to read here.
//!
//! The turn itself is behind [`ImageTurnRunner`], which the app implements on
//! top of its Antigravity provider.
pub mod gallery;
pub mod styles;

use crate::time::now_ms;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Takes a single request may ask for.
pub const MAX_TAKES: usize = 4;
pub const ASPECTS: [&str; 10] = [
    "1:1", "4:5", "5:4", "3:2", "2:3", "4:3", "3:4", "16:9", "9:16", "21:9",
];
const MAX_PROMPT_CHARS: usize = 8_000;
/// Folder under the app's data directory that keeps the pictures.
pub const GENERATED_IMAGE_DIR: &str = "generated-images";
/// Larger pictures are skipped when collecting (`MAX_GENERATED_IMAGE_BYTES`).
pub const MAX_IMAGE_BYTES: u64 = 25 * 1024 * 1024;
/// The model every image turn runs on (`DEFAULT_MODEL_ID.antigravity`).
pub const IMAGE_MODEL: &str = "antigravity:gemini-3.8-flash-high";
/// Where Antigravity writes a conversation's files, relative to the home folder.
pub const BRAIN_DIR: &str = ".gemini/antigravity-cli/brain";

pub const ONLY_MONO_ERROR: &str = "Only a Mono's own chat can make pictures";
pub const IMAGES_OFF_ERROR: &str = "Images is off in your tools. Ask the user to turn it on, or turn it on with tools.update if they asked you to.";
const NOTE: &str = "Frames for the pictures are in your chat now and the pictures replace them when Gemini is done, usually within a minute or two. Don't wait or poll for them: end your turn with one short line on what you asked for, and let the user react.";

/// What a Mono asks for with `app images.generate`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageRequest {
    pub prompt: String,
    /// Takes to make, 1 to 4.
    pub count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect: Option<String>,
    /// An image to edit, by absolute path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// A preset look's id ([`styles::STYLES`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
}

fn absolute_path_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(/|[A-Za-z]:[\\/])").unwrap())
}

/// JavaScript's `String(value)` for the JSON values a Mono can send.
fn js_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

/// JavaScript's `Number(value)` as far as the count check needs it.
fn js_number(value: &Value) -> f64 {
    match value {
        Value::Number(number) => number.as_f64().unwrap_or(f64::NAN),
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                0.0
            } else {
                text.parse().unwrap_or(f64::NAN)
            }
        }
        Value::Bool(flag) => f64::from(u8::from(*flag)),
        Value::Null => 0.0,
        _ => f64::NAN,
    }
}

/// Reads `images.generate` input, or says what is wrong with it
/// (`parseImageRequest`).
pub fn parse_request(input: &Value) -> Result<ImageRequest, String> {
    let prompt = input
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if prompt.is_empty() || prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err("prompt must be a non-empty string under 8000 characters".into());
    }
    let count_error = || format!("count must be a whole number from 1 to {MAX_TAKES}");
    let count = match input.get("count") {
        None => 1,
        Some(value) => {
            let number = js_number(value);
            if !(number.fract() == 0.0 && number >= 1.0 && number <= MAX_TAKES as f64) {
                return Err(count_error());
            }
            number as usize
        }
    };
    // A named format ("portrait", "/story") stands for its ratio.
    let aspect = input.get("aspect").map(js_string).or_else(|| {
        input
            .get("format")
            .map(js_string)
            .map(|id| styles::format(&id).map_or(id, |format| format.aspect.to_owned()))
    });
    if let Some(aspect) = &aspect {
        if !ASPECTS.contains(&aspect.as_str()) {
            return Err(format!("aspect must be one of {}", ASPECTS.join(", ")));
        }
    }
    let style = match input.get("style").map(js_string) {
        None => None,
        Some(id) if id.trim().is_empty() => None,
        Some(id) => Some(
            styles::style(&id)
                .ok_or_else(|| {
                    format!(
                        "style must be one of {}",
                        styles::STYLES
                            .iter()
                            .map(|style| style.id)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })?
                .id
                .to_owned(),
        ),
    };
    let source = input.get("source").map(js_string);
    if let Some(source) = &source {
        if !absolute_path_re().is_match(source) {
            return Err("source must be the absolute path of an image".into());
        }
    }
    Ok(ImageRequest {
        prompt: prompt.to_string(),
        count,
        aspect: aspect.filter(|value| !value.is_empty()),
        source: source.filter(|value| !value.is_empty()),
        style,
    })
}

/// The tools a bot role has when it has no choice of its own (`BOT_ROLES`).
/// Only the images bot gets pictures by default.
pub fn role_access(role: &str) -> &'static [&'static str] {
    match role {
        "inbox" | "phone" => &["read"],
        "writer" | "documents" => &["web", "read", "edit"],
        "analyst" => &["read", "edit"],
        "images" => &["web", "read", "images"],
        "social" => &["web", "read"],
        _ => &["web", "read"],
    }
}

/// Whether a bot may make pictures: its own choice of Claude tools when it
/// has one, else its role's (`botTools(...).access.includes("images")`).
pub fn bot_may_make_images<S: AsRef<str>>(role: &str, own_access: Option<&[S]>) -> bool {
    match own_access {
        Some(own) => own.iter().any(|access| access.as_ref() == "images"),
        None => role_access(role).contains(&"images"),
    }
}

/// Who may call `images.generate`: a Mono's own chat, and a bot only with
/// Images in its tools. `role` is the bot's role (`None` for a plain Mono)
/// and `own_access` the access list the user set on it, when it has one.
pub fn check_access<S: AsRef<str>>(
    is_mono: bool,
    bot_role: Option<&str>,
    own_access: Option<&[S]>,
) -> Result<(), String> {
    if !is_mono {
        return Err(ONLY_MONO_ERROR.into());
    }
    match bot_role {
        Some(role) if !bot_may_make_images(role, own_access) => Err(IMAGES_OFF_ERROR.into()),
        _ => Ok(()),
    }
}

/// The JSON the action returns once the job has started.
pub fn action_response(job_id: &str, request: &ImageRequest) -> Value {
    json!({ "started": job_id, "takes": request.count, "note": NOTE })
}

/// The turn Gemini gets: make exactly the pictures asked for, nothing else
/// (`imageTurnText`).
pub fn turn_text(request: &ImageRequest) -> String {
    let takes = if request.count == 1 {
        "one picture".to_string()
    } else {
        format!(
            "{} pictures, each a different take on the brief",
            request.count
        )
    };
    let mut parts = vec![format!(
        "Use your generate_image tool to make {takes}. Do nothing else: no files, no shell, no questions; when the pictures are made, reply with one short line."
    )];
    if let Some(aspect) = &request.aspect {
        parts.push(format!("Aspect ratio: {aspect}."));
    }
    if let Some(style) = request.style.as_deref().and_then(styles::style) {
        parts.push(format!(
            "Style: render it as {}. Keep this look across every take.",
            style.direction
        ));
    }
    if let Some(source) = &request.source {
        parts.push(format!(
            "Edit the attached image ({source}); keep what the brief does not change."
        ));
    }
    parts.push(format!("Brief:\n{}", request.prompt));
    parts.join("\n\n")
}

/// Only the picture tool runs unattended; anything else Gemini tries is
/// refused (`allowsImageTool`).
pub fn allows_image_tool(title: &str, kind: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)generate_image|image[-_ ]?generat|nano.?banana").unwrap())
        .is_match(&format!("{title} {kind}"))
}

/// The transcript role of a picture a provider made during an ordinary turn
/// (Codex's `imageGeneration`, Antigravity's `generate_image`): a
/// [`GeneratedImage`] as JSON (`image.generated`).
pub const REPLY_ROLE: &str = "image";

/// Whether a provider update shows the turn asking Gemini for a picture
/// (`updateMakesImage`): its picture tool, or the subagent it hands them to.
pub fn update_makes_image(update: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)generate_image|image[-_]generator").unwrap())
        .is_match(update)
}

/// Saves a picture a provider returned as base64 (optionally a `data:` URL)
/// into `dir`, after checking that its bytes are an image
/// (`materializeGeneratedImage`).
pub fn save_base64(
    data: &str,
    name: &str,
    alt: Option<String>,
    dir: &Path,
) -> Result<GeneratedImage, String> {
    use base64::Engine;
    let payload = data
        .trim()
        .split_once("base64,")
        .map_or(data.trim(), |(_, payload)| payload);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.as_bytes())
        .map_err(|_| "The picture is not valid base64".to_string())?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err("The picture is empty or too large".into());
    }
    let (extension, mime_type) = match image::guess_format(&bytes) {
        Ok(image::ImageFormat::Png) => ("png", "image/png"),
        Ok(image::ImageFormat::Jpeg) => ("jpg", "image/jpeg"),
        Ok(image::ImageFormat::WebP) => ("webp", "image/webp"),
        Ok(image::ImageFormat::Gif) => ("gif", "image/gif"),
        _ => return Err("The picture is not a PNG, JPEG, WebP or GIF image".into()),
    };
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stem = safe_file_name(name);
    let stem = stem
        .rsplit_once('.')
        .map_or(stem.as_str(), |(stem, _)| stem);
    let path = dir.join(format!("{}-{stem}.{extension}", uuid::Uuid::new_v4()));
    std::fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(GeneratedImage {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: path.to_string_lossy().into_owned(),
        mime_type: mime_type.into(),
        size: bytes.len() as u64,
        alt: alt.filter(|alt| !alt.trim().is_empty()),
    })
}

/// A picture on disk (`GeneratedImageMeta`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedImage {
    pub path: String,
    pub name: String,
    pub mime_type: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Generating,
    Done,
    Failed,
}

/// Pictures a Mono asked Gemini for: frames while they are made, then the
/// images (`ImageJobMeta`). It is the data of an `image` transcript block; the
/// block's id is the job's id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageJob {
    pub id: String,
    pub prompt: String,
    /// Takes asked for, so the frames match before the images land.
    pub count: usize,
    /// "16:9" and the like; the frames take its shape. Square when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect: Option<String>,
    /// The preset look it was made in, by id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    pub status: JobStatus,
    /// Unix milliseconds.
    pub started_at: u64,
    pub images: Vec<GeneratedImage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ImageJob {
    /// The placeholder the chat shows right away.
    pub fn start(id: impl Into<String>, request: &ImageRequest) -> Self {
        Self {
            id: id.into(),
            prompt: request.prompt.clone(),
            count: request.count,
            aspect: request.aspect.clone(),
            style: request.style.clone(),
            status: JobStatus::Generating,
            started_at: now_ms(),
            images: Vec::new(),
            error: None,
        }
    }

    /// Swaps the frames for the pictures.
    pub fn done(&self, images: Vec<GeneratedImage>) -> Self {
        Self {
            status: JobStatus::Done,
            images,
            ..self.clone()
        }
    }

    pub fn failed(&self, error: impl Into<String>) -> Self {
        Self {
            status: JobStatus::Failed,
            error: Some(error.into()),
            ..self.clone()
        }
    }

    /// Files this job owns, for removing a session.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.images.iter().map(|image| image.path.as_str())
    }
}

/// A fresh job id.
pub fn new_job_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// What an Antigravity image turn reports while it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnEvent {
    /// A picture on disk (`image.generated`).
    Image(GeneratedImage),
    /// Gemini asks to run a tool (`approval.requested`); the callback's
    /// return value answers it.
    Approval { title: String, kind: String },
    /// The turn failed (`session.error`).
    Error(String),
}

/// One turn to run on a throwaway Antigravity session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageTurn {
    pub text: String,
    pub model: String,
    /// The picture to edit, as an attachment.
    pub source: Option<PathBuf>,
}

/// Runs a turn on its own ephemeral Antigravity session (supervised, in the
/// home folder), reports its events through `on_event` (returning whether to
/// allow an approval) and forgets the session afterwards. Blocking.
pub trait ImageTurnRunner {
    fn run(
        &self,
        turn: &ImageTurn,
        on_event: &mut dyn FnMut(TurnEvent) -> bool,
    ) -> Result<(), String>;
}

/// Runs one turn and returns the pictures it made (`runImageTurn`).
pub fn run_image_turn(
    request: &ImageRequest,
    runner: &dyn ImageTurnRunner,
) -> Result<Vec<GeneratedImage>, String> {
    let turn = ImageTurn {
        text: turn_text(request),
        model: IMAGE_MODEL.into(),
        source: request.source.as_ref().map(PathBuf::from),
    };
    let mut images = Vec::new();
    let mut failure = None;
    let result = runner.run(&turn, &mut |event| match event {
        TurnEvent::Image(image) => {
            images.push(image);
            false
        }
        TurnEvent::Approval { title, kind } => allows_image_tool(&title, &kind),
        TurnEvent::Error(message) => {
            failure = Some(message);
            false
        }
    });
    if images.is_empty() {
        return Err(failure.or(result.err()).unwrap_or_else(|| {
            "Gemini made no pictures. Check that Antigravity (agy) is installed and signed in."
                .into()
        }));
    }
    images.truncate(request.count.max(1));
    Ok(images)
}

/// Makes the pictures and returns where they are: inside `save_dir`, copied
/// there when the runner left them elsewhere. Blocking, for a background
/// thread. There is no API key: Antigravity signs in on its own.
pub fn generate(
    request: &ImageRequest,
    runner: &dyn ImageTurnRunner,
    save_dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    if let Some(source) = &request.source {
        if !Path::new(source).is_file() {
            return Err(format!("source image not found: {source}"));
        }
    }
    let images = run_image_turn(request, runner)?;
    std::fs::create_dir_all(save_dir).map_err(|e| format!("{}: {e}", save_dir.display()))?;
    images
        .iter()
        .map(|image| keep_in(Path::new(&image.path), &image.name, save_dir))
        .collect()
}

fn keep_in(path: &Path, name: &str, save_dir: &Path) -> Result<PathBuf, String> {
    if path.parent() == Some(save_dir) {
        return Ok(path.to_path_buf());
    }
    let target = save_dir.join(format!("{}-{}", uuid::Uuid::new_v4(), safe_file_name(name)));
    std::fs::copy(path, &target).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(target)
}

/// A file name that is safe to create (`safe_attachment_name`).
pub fn safe_file_name(name: &str) -> String {
    let leaf = Path::new(name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment");
    let cleaned: String = leaf
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('.').trim_matches('-');
    if trimmed.is_empty() {
        "attachment".into()
    } else {
        trimmed.chars().take(80).collect()
    }
}

fn image_mime(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// Antigravity's `generate_image` saves its pictures in the conversation's
/// folder under [`BRAIN_DIR`], not in the reply. Copies the ones written
/// since `since_ms` into `destination`: at most four, distinct by content
/// (`collect_images_since`).
pub fn collect_images_since(
    brain: &Path,
    destination: &Path,
    since_ms: u64,
) -> Result<Vec<GeneratedImage>, String> {
    let since = UNIX_EPOCH + Duration::from_millis(since_ms);
    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
    let Ok(conversations) = std::fs::read_dir(brain) else {
        return Ok(Vec::new());
    };
    for conversation in conversations.flatten() {
        // Only recently touched conversations can hold this turn's images.
        let touched = conversation.metadata().and_then(|meta| meta.modified());
        if touched.map(|at| at < since).unwrap_or(true) {
            continue;
        }
        let Ok(files) = std::fs::read_dir(conversation.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if image_mime(&path).is_none() {
                continue;
            }
            let Ok(meta) = file.metadata() else { continue };
            if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_IMAGE_BYTES {
                continue;
            }
            if let Ok(modified) = meta.modified() {
                if modified >= since {
                    found.push((modified, path));
                }
            }
        }
    }
    found.sort();
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    let mut collected = Vec::new();
    for (_, source) in found {
        if collected.len() >= MAX_TAKES {
            break;
        }
        let Ok(bytes) = std::fs::read(&source) else {
            continue;
        };
        if !seen.insert(bytes.clone()) {
            continue;
        }
        let mime = image_mime(&source).unwrap_or("image/png");
        let name = source
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("image.png")
            .to_string();
        std::fs::create_dir_all(destination).map_err(|e| e.to_string())?;
        let target = destination.join(format!(
            "{}-{}",
            uuid::Uuid::new_v4(),
            safe_file_name(&name)
        ));
        std::fs::write(&target, &bytes).map_err(|e| format!("{}: {e}", target.display()))?;
        collected.push(GeneratedImage {
            path: target.to_string_lossy().into_owned(),
            name,
            mime_type: mime.into(),
            size: bytes.len() as u64,
            alt: None,
        });
    }
    Ok(collected)
}

/// The pictures a turn made, once they are on disk. Gemini's image subagent
/// can still be writing when the turn ends, so this looks again every
/// `every` until one shows up or `wait` passes (`awaitTurnImages`; the
/// defaults there are two minutes and two seconds).
pub fn await_turn_images(
    wait: Duration,
    every: Duration,
    stopped: impl Fn() -> bool,
    mut collect: impl FnMut() -> Result<Vec<GeneratedImage>, String>,
    sleep: impl Fn(Duration),
) -> Vec<GeneratedImage> {
    let started = std::time::Instant::now();
    loop {
        if stopped() {
            return Vec::new();
        }
        if let Ok(found) = collect() {
            if !found.is_empty() {
                return found;
            }
        }
        if started.elapsed() + every > wait {
            return Vec::new();
        }
        sleep(every);
    }
}

/// The app's Antigravity provider as the image turn's runner: a throwaway
/// supervised session in the home folder, the picture tool allowed and
/// everything else refused, then the pictures Gemini left in its brain
/// folder (`generateImages` on the Antigravity harness).
pub struct AntigravityRunner {
    pub binary: PathBuf,
    pub home: PathBuf,
    /// Where the collected pictures are copied.
    pub save_dir: PathBuf,
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl AntigravityRunner {
    /// The installed Antigravity and the user's home, or why it can't run.
    pub fn installed(save_dir: PathBuf) -> Result<Self, String> {
        use crate::provider::Kind;
        let binary = crate::provider::resolve(Kind::Antigravity)
            .ok_or_else(|| Kind::Antigravity.unavailable_hint())?;
        let home = dirs::home_dir().ok_or("No home folder for Antigravity")?;
        Ok(Self {
            binary,
            home,
            save_dir,
            cancel: Default::default(),
        })
    }
}

impl ImageTurnRunner for AntigravityRunner {
    fn run(
        &self,
        turn: &ImageTurn,
        on_event: &mut dyn FnMut(TurnEvent) -> bool,
    ) -> Result<(), String> {
        use crate::agent::{AccessMode, Control, Decision, Settings};
        use crate::provider::{Event, Kind, Request};
        use std::sync::atomic::Ordering;
        let started = now_ms();
        let (controls, answers) = std::sync::mpsc::channel();
        let model = turn
            .model
            .split_once(':')
            .map_or(turn.model.as_str(), |(_, id)| id);
        let request = Request {
            kind: Kind::Antigravity,
            binary: self.binary.clone(),
            cwd: self.home.clone(),
            prompt: turn.text.clone(),
            provider_id: None,
            model: Some(model.into()),
            settings: Settings {
                access: AccessMode::Supervised,
                attachments: turn.source.iter().cloned().collect(),
                ..Default::default()
            },
        };
        let result = crate::provider::run_with_controls(
            request,
            self.cancel.clone(),
            Some(answers),
            |events| {
                for event in events {
                    if let Event::Approval(approval) = event {
                        let allow = on_event(TurnEvent::Approval {
                            title: approval.title,
                            kind: approval.detail,
                        });
                        let decision = if allow {
                            Decision::Allow
                        } else {
                            Decision::Deny
                        };
                        let _ = controls.send(Control {
                            id: approval.id,
                            decision,
                        });
                    }
                }
                Ok(())
            },
        );
        if let Err(error) = &result {
            on_event(TurnEvent::Error(error.clone()));
        }
        // Gemini's image subagent can still be writing when the turn ends.
        let brain = self.home.join(BRAIN_DIR);
        let wait = if result.is_ok() { 120 } else { 4 };
        for image in await_turn_images(
            Duration::from_secs(wait),
            Duration::from_secs(2),
            || self.cancel.load(Ordering::Relaxed),
            || collect_images_since(&brain, &self.save_dir, started),
            std::thread::sleep,
        ) {
            on_event(TurnEvent::Image(image));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn parses_the_input_like_the_tauri_app() {
        assert_eq!(
            parse_request(&json!({ "prompt": " a fox " })).unwrap(),
            ImageRequest {
                prompt: "a fox".into(),
                count: 1,
                aspect: None,
                source: None,
                style: None,
            }
        );
        let full = parse_request(&json!({
            "prompt": "a fox", "count": 3, "aspect": "16:9", "source": "C:\\pics\\fox.png"
        }))
        .unwrap();
        assert_eq!(full.count, 3);
        assert_eq!(full.aspect.as_deref(), Some("16:9"));
        assert_eq!(full.source.as_deref(), Some("C:\\pics\\fox.png"));
        assert_eq!(
            parse_request(&json!({ "prompt": "x", "count": "2" }))
                .unwrap()
                .count,
            2
        );
        assert!(parse_request(&json!({ "prompt": "x", "source": "/a/b.png" })).is_ok());
    }

    #[test]
    fn rejects_bad_input_with_the_same_words() {
        let err = |v: Value| parse_request(&v).unwrap_err();
        assert!(err(json!({ "prompt": "" })).contains("prompt"));
        assert!(err(json!({ "prompt": "  " })).contains("prompt"));
        assert!(err(json!({})).contains("prompt"));
        assert!(err(json!({ "prompt": "x".repeat(8_001) })).contains("prompt"));
        assert_eq!(
            err(json!({ "prompt": "x", "count": 9 })),
            "count must be a whole number from 1 to 4"
        );
        assert!(err(json!({ "prompt": "x", "count": 0 })).contains("count"));
        assert!(err(json!({ "prompt": "x", "count": 1.5 })).contains("count"));
        assert!(err(json!({ "prompt": "x", "count": "many" })).contains("count"));
        assert_eq!(
            err(json!({ "prompt": "x", "aspect": "5:1" })),
            "aspect must be one of 1:1, 4:5, 5:4, 3:2, 2:3, 4:3, 3:4, 16:9, 9:16, 21:9"
        );
        assert_eq!(
            err(json!({ "prompt": "x", "source": "a.png" })),
            "source must be the absolute path of an image"
        );
    }

    #[test]
    fn takes_a_preset_style_and_a_named_format() {
        let request = parse_request(&json!({
            "prompt": "a fox", "style": "/Screenprint", "format": "portrait"
        }))
        .unwrap();
        assert_eq!(request.style.as_deref(), Some("screenprint"));
        assert_eq!(request.aspect.as_deref(), Some("4:5"));
        // An explicit aspect wins over the format.
        let both =
            parse_request(&json!({ "prompt": "x", "aspect": "16:9", "format": "story" })).unwrap();
        assert_eq!(both.aspect.as_deref(), Some("16:9"));
        assert!(parse_request(&json!({ "prompt": "x", "format": "huge" }))
            .unwrap_err()
            .starts_with("aspect must be one of"));
        assert!(
            parse_request(&json!({ "prompt": "x", "style": "vaporwave" }))
                .unwrap_err()
                .starts_with("style must be one of photoreal, cinematic")
        );
        assert_eq!(
            parse_request(&json!({ "prompt": "x", "style": " " }))
                .unwrap()
                .style,
            None
        );
        // The brief carries the look, and the job remembers it.
        let text = turn_text(&request);
        assert!(text.contains("Aspect ratio: 4:5."));
        assert!(text.contains("Style: render it as a vintage screenprint poster"));
        let job = ImageJob::start("j", &request);
        assert_eq!(serde_json::to_value(&job).unwrap()["style"], "screenprint");
    }

    #[test]
    fn only_the_images_bot_makes_pictures_by_default() {
        let none: Option<&[String]> = None;
        assert!(check_access(true, None, none).is_ok());
        assert!(check_access(true, Some("images"), none).is_ok());
        assert_eq!(
            check_access(true, Some("assistant"), none).unwrap_err(),
            IMAGES_OFF_ERROR
        );
        assert_eq!(
            check_access(true, Some("custom"), none).unwrap_err(),
            IMAGES_OFF_ERROR
        );
        // The bot's own list wins over its role's, both ways.
        let own = ["web", "images"];
        assert!(check_access(true, Some("assistant"), Some(&own[..])).is_ok());
        let without = ["web", "read"];
        assert!(check_access(true, Some("images"), Some(&without[..])).is_err());
        assert_eq!(
            check_access(false, None, none).unwrap_err(),
            ONLY_MONO_ERROR
        );
    }

    #[test]
    fn answers_with_started_takes_and_note() {
        let request = parse_request(&json!({ "prompt": "x", "count": 2 })).unwrap();
        let answer = action_response("job-1", &request);
        assert_eq!(answer["started"], "job-1");
        assert_eq!(answer["takes"], 2);
        assert!(answer["note"]
            .as_str()
            .unwrap()
            .starts_with("Frames for the pictures are in your chat now"));
        assert_eq!(answer.as_object().unwrap().len(), 3);
    }

    #[test]
    fn briefs_gemini_for_one_take_or_many_and_for_edits() {
        let one = turn_text(&parse_request(&json!({ "prompt": "a fox" })).unwrap());
        assert!(one.starts_with("Use your generate_image tool to make one picture."));
        assert!(one.ends_with("Brief:\na fox"));
        assert!(!one.contains("Aspect ratio"));
        let many = turn_text(
            &parse_request(&json!({
                "prompt": "a fox", "count": 3, "aspect": "3:2", "source": "/tmp/fox.png"
            }))
            .unwrap(),
        );
        assert!(many.contains("3 pictures, each a different take on the brief"));
        assert!(many.contains("\n\nAspect ratio: 3:2.\n\n"));
        assert!(many.contains(
            "Edit the attached image (/tmp/fox.png); keep what the brief does not change."
        ));
    }

    #[test]
    fn approves_only_the_picture_tool() {
        assert!(allows_image_tool("generate_image", ""));
        assert!(allows_image_tool("Run", "image-generator"));
        assert!(allows_image_tool("Nano Banana", ""));
        assert!(allows_image_tool("NanoBanana", ""));
        assert!(!allows_image_tool("run_command", "execute"));
        assert!(!allows_image_tool("", ""));
    }

    #[test]
    fn a_job_goes_from_frames_to_pictures_or_an_error() {
        let request =
            parse_request(&json!({ "prompt": "a fox", "count": 2, "aspect": "16:9" })).unwrap();
        let job = ImageJob::start(new_job_id(), &request);
        assert_eq!(job.status, JobStatus::Generating);
        assert!(job.images.is_empty());
        let image = GeneratedImage {
            path: "/a/b.png".into(),
            name: "b.png".into(),
            mime_type: "image/png".into(),
            size: 3,
            alt: None,
        };
        let done = job.done(vec![image]);
        assert_eq!(done.status, JobStatus::Done);
        assert_eq!(done.paths().collect::<Vec<_>>(), ["/a/b.png"]);
        let failed = job.failed("nope");
        assert_eq!(failed.status, JobStatus::Failed);
        assert_eq!(failed.error.as_deref(), Some("nope"));
        // Same shape as the Tauri transcript's `imageJob`.
        let value = serde_json::to_value(&done).unwrap();
        assert_eq!(value["status"], "done");
        assert_eq!(value["aspect"], "16:9");
        assert_eq!(value["images"][0]["mimeType"], "image/png");
        assert!(value.get("error").is_none() && value.get("startedAt").is_some());
        assert_eq!(serde_json::from_value::<ImageJob>(value).unwrap(), done);
    }

    struct Fake {
        events: Vec<TurnEvent>,
        approved: RefCell<Vec<bool>>,
        seen: RefCell<Option<ImageTurn>>,
    }
    impl Fake {
        fn new(events: Vec<TurnEvent>) -> Self {
            Self {
                events,
                approved: RefCell::default(),
                seen: RefCell::default(),
            }
        }
    }
    impl ImageTurnRunner for Fake {
        fn run(
            &self,
            turn: &ImageTurn,
            on_event: &mut dyn FnMut(TurnEvent) -> bool,
        ) -> Result<(), String> {
            *self.seen.borrow_mut() = Some(turn.clone());
            for event in &self.events {
                let approval = matches!(event, TurnEvent::Approval { .. });
                let answer = on_event(event.clone());
                if approval {
                    self.approved.borrow_mut().push(answer);
                }
            }
            Ok(())
        }
    }

    fn image(path: &Path, name: &str) -> GeneratedImage {
        GeneratedImage {
            path: path.to_string_lossy().into_owned(),
            name: name.into(),
            mime_type: "image/png".into(),
            size: 1,
            alt: None,
        }
    }

    #[test]
    fn a_turn_keeps_at_most_the_takes_asked_for_and_gates_tools() {
        let dir = tempfile::tempdir().unwrap();
        let events = vec![
            TurnEvent::Approval {
                title: "run_command".into(),
                kind: "execute".into(),
            },
            TurnEvent::Approval {
                title: "generate_image".into(),
                kind: "other".into(),
            },
            TurnEvent::Image(image(&dir.path().join("a.png"), "a.png")),
            TurnEvent::Image(image(&dir.path().join("b.png"), "b.png")),
        ];
        let fake = Fake::new(events);
        let request = parse_request(&json!({ "prompt": "x" })).unwrap();
        let images = run_image_turn(&request, &fake).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(*fake.approved.borrow(), [false, true]);
        let turn = fake.seen.borrow().clone().unwrap();
        assert_eq!(turn.model, IMAGE_MODEL);
        assert_eq!(turn.text, turn_text(&request));
    }

    #[test]
    fn no_pictures_says_why() {
        let request = parse_request(&json!({ "prompt": "x" })).unwrap();
        let failing = Fake::new(vec![TurnEvent::Error("quota".into())]);
        assert_eq!(run_image_turn(&request, &failing).unwrap_err(), "quota");
        let silent = Fake::new(vec![]);
        assert!(run_image_turn(&request, &silent)
            .unwrap_err()
            .contains("Antigravity (agy)"));
    }

    #[test]
    fn generate_returns_paths_inside_the_save_dir() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = root.path().join("brain-copy.png");
        std::fs::write(&elsewhere, b"png").unwrap();
        let save = root.path().join("out");
        let fake = Fake::new(vec![TurnEvent::Image(image(&elsewhere, "my pic.png"))]);
        let request = parse_request(&json!({ "prompt": "x" })).unwrap();
        let paths = generate(&request, &fake, &save).unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].starts_with(&save) && paths[0].is_file());
        assert!(paths[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("-my-pic.png"));
        // A picture already in the folder stays where it is.
        let inside = save.join("kept.png");
        std::fs::write(&inside, b"png").unwrap();
        let again = Fake::new(vec![TurnEvent::Image(image(&inside, "kept.png"))]);
        assert_eq!(generate(&request, &again, &save).unwrap(), [inside]);
        // A missing source fails before the turn runs.
        let edit = parse_request(&json!({ "prompt": "x", "source": root.path().join("none.png") }))
            .unwrap();
        let never = Fake::new(vec![]);
        assert!(generate(&edit, &never, &save)
            .unwrap_err()
            .starts_with("source image not found"));
        assert!(never.seen.borrow().is_none());
    }

    #[test]
    fn collects_only_new_distinct_pictures_from_the_brain() {
        let root = tempfile::tempdir().unwrap();
        let brain = root.path().join("brain");
        let conversation = brain.join("conv");
        std::fs::create_dir_all(&conversation).unwrap();
        let since = now_ms() - 1_000;
        std::fs::write(conversation.join("fox.jpg"), b"one").unwrap();
        std::fs::write(conversation.join("fox_copy.jpg"), b"one").unwrap();
        std::fs::write(conversation.join("cat.png"), b"two").unwrap();
        std::fs::write(conversation.join("notes.md"), b"text").unwrap();
        let out = root.path().join("out");
        let images = collect_images_since(&brain, &out, since).unwrap();
        assert_eq!(images.len(), 2);
        assert!(images.iter().any(|image| image.mime_type == "image/jpeg"));
        assert!(images
            .iter()
            .all(|image| Path::new(&image.path).starts_with(&out)));
        // Nothing is new after now, and a missing brain is not an error.
        assert!(collect_images_since(&brain, &out, since + 3_600_000)
            .unwrap()
            .is_empty());
        assert!(collect_images_since(&root.path().join("none"), &out, 0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn waits_for_late_pictures_then_gives_up() {
        let calls = RefCell::new(0);
        let late = await_turn_images(
            Duration::from_secs(60),
            Duration::from_millis(1),
            || false,
            || {
                *calls.borrow_mut() += 1;
                Ok(if *calls.borrow() == 3 {
                    vec![image(Path::new("/x.png"), "x.png")]
                } else {
                    vec![]
                })
            },
            |_| {},
        );
        assert_eq!(late.len(), 1);
        assert_eq!(*calls.borrow(), 3);
        let never = await_turn_images(
            Duration::from_millis(5),
            Duration::from_millis(2),
            || false,
            || Ok(vec![]),
            std::thread::sleep,
        );
        assert!(never.is_empty());
        let stopped = await_turn_images(
            Duration::from_secs(60),
            Duration::from_millis(1),
            || true,
            || Ok(vec![image(Path::new("/x.png"), "x.png")]),
            |_| {},
        );
        assert!(stopped.is_empty());
    }

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(safe_file_name("../a b/c d.png"), "c-d.png");
        assert_eq!(safe_file_name("..."), "attachment");
        assert_eq!(safe_file_name(&"x".repeat(200)).len(), 80);
    }
}

#[cfg(test)]
mod reply_tests {
    use super::*;
    use base64::Engine;

    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn a_base64_picture_is_saved_as_the_image_it_is() {
        let dir = std::env::temp_dir().join(format!("monocode-reply-{}", uuid::Uuid::new_v4()));
        let data = base64::engine::general_purpose::STANDARD.encode(png());
        let saved =
            save_base64(&data, "generated-image", Some("A red square".into()), &dir).unwrap();
        assert!(saved.name.ends_with("-generated-image.png"));
        assert_eq!(saved.mime_type, "image/png");
        assert_eq!(saved.alt.as_deref(), Some("A red square"));
        assert_eq!(std::fs::read(&saved.path).unwrap(), png());
        let url = format!("data:image/png;base64,{data}");
        assert!(save_base64(&url, "x", Some(" ".into()), &dir)
            .unwrap()
            .alt
            .is_none());
        let text = base64::engine::general_purpose::STANDARD.encode(b"not a picture");
        assert!(save_base64(&text, "x", None, &dir).is_err());
        assert!(save_base64("%%%", "x", None, &dir).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn picture_updates_are_recognised() {
        assert!(update_makes_image(r#"{"title":"generate_image"}"#));
        assert!(update_makes_image(r#"{"agent":"Image-Generator"}"#));
        assert!(!update_makes_image(r#"{"title":"Read image.png"}"#));
    }
}
