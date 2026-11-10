//! Live voice for monoBOT's calls, as IVY runs it
//! (`IVY/crates/voice/src/{gemini,gemini_live,phonic,phonic_live}.rs`): a
//! model that hears the user and answers out loud in real time, and asks the
//! user's agents through a tool when it needs them.
//!
//! - `gemini` and `phonic` turn the wire protocol into plain values and back,
//!   with no network, so every message is tested.
//! - `session` runs one conversation on its own thread over a `Transport`:
//!   setup, audio both ways, interruptions, tools that may take minutes, and
//!   reconnection when Gemini asks to move.
//! - `transport` is the real WebSocket; tests use a channel instead.
//!
//! The API key travels only in a request header, never in a URL, and is never
//! logged.
pub mod gemini;
pub mod phonic;
pub mod session;
pub mod transport;

use serde_json::Value;

/// What the far end said, decoded.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The setup was accepted: audio may flow.
    Ready,
    /// Speech to play, mono f32 at the protocol's output rate.
    Audio(Vec<f32>),
    /// Words the model heard from the user so far in this turn.
    Heard(String),
    /// Words the model is saying.
    Said(String),
    /// The user talked over the model: drop what is still queued to play.
    Interrupted,
    /// The model finished its turn.
    TurnDone,
    /// The model wants a tool run.
    Tool(ToolCall),
    /// The model no longer wants these tool calls (by id).
    Cancelled(Vec<String>),
    /// The server will close soon; reconnect at the next quiet moment.
    GoAway,
    /// A handle to resume this conversation on a new connection.
    Resumable(String),
    /// The far end refused or failed, in the user's words.
    Error(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

/// A protocol's side of the conversation.
pub trait Protocol: Send + 'static {
    fn url(&self) -> &str;
    /// The request headers, the API key among them.
    fn headers(&self) -> Vec<(&'static str, String)>;
    /// The first message, resuming `handle` when there is one.
    fn setup(&self, handle: Option<&str>) -> String;
    /// Microphone audio (mono, `INPUT_RATE`), as one or more messages.
    fn audio(&self, samples: &[f32]) -> Vec<String>;
    /// The answer to a tool call that finished in time.
    fn tool_result(&self, call: &ToolCall, result: &Value) -> String;
    /// What tells the model a slow tool is still running, if the protocol
    /// says anything meanwhile.
    fn still_running(&self, call: &ToolCall) -> Option<String>;
    /// How a slow tool's result reaches the model after `still_running`.
    fn late_result(&self, call: &ToolCall, result: &Value) -> String;
    /// Decodes one message from the far end.
    fn parse(&self, message: &str) -> Vec<Event>;
    /// The rate speech arrives at.
    fn output_rate(&self) -> u32;
}

/// The rate both protocols take the microphone at.
pub const INPUT_RATE: u32 = 16_000;

/// f32 samples as 16-bit little-endian PCM.
pub fn pcm16(samples: &[f32]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| ((sample.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes())
        .collect()
}

/// 16-bit little-endian PCM as f32 samples; a trailing odd byte is dropped.
pub fn from_pcm16(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair) as f32 / 32768.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_round_trips_and_clips() {
        let samples = [0.0, 0.5, -0.5, 1.0, -1.0, 2.0];
        let bytes = pcm16(&samples);
        assert_eq!(bytes.len(), samples.len() * 2);
        let back = from_pcm16(&bytes);
        for (got, want) in back.iter().zip([0.0, 0.5, -0.5, 1.0, -1.0, 1.0]) {
            assert!((got - want).abs() < 0.001, "{got} {want}");
        }
        assert_eq!(from_pcm16(&[1, 2, 3]).len(), 1);
    }
}
