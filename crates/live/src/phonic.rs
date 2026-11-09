//! Phonic speech-to-speech: `wss://api.phonic.ai/v1/sts/ws` with a Bearer
//! key. Audio goes up as 16 kHz PCM in 20 ms chunks and comes back at
//! 44.1 kHz, ahead of real time. Its tools are synchronous: the model waits
//! for each answer (up to two minutes), so a slow one only gets a spoken
//! "still working" meanwhile.
use crate::{Event, Protocol, ToolCall, from_pcm16, gemini::Tool, pcm16};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

pub const URL: &str = "wss://api.phonic.ai/v1/sts/ws";
pub const OUTPUT_RATE: u32 = 44_100;
const FRAME: usize = 320;

pub struct Phonic {
    key: String,
    pub voice: String,
    pub language: String,
    pub instructions: String,
    pub tools: Vec<Tool>,
}

impl Phonic {
    pub fn new(key: &str, instructions: &str, tools: Vec<Tool>) -> Self {
        Self {
            key: key.to_owned(),
            voice: "sabrina".into(),
            language: "es".into(),
            instructions: instructions.to_owned(),
            tools,
        }
    }
}

impl Protocol for Phonic {
    fn url(&self) -> &str {
        URL
    }

    fn headers(&self) -> Vec<(&'static str, String)> {
        vec![("Authorization", format!("Bearer {}", self.key))]
    }

    fn setup(&self, _: Option<&str>) -> String {
        let tools: Vec<Value> = self
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "custom_websocket",
                    "execution_mode": "sync",
                    "require_speech_before_tool_call": true,
                    "tool_call_output_timeout_ms": 120_000,
                    "tool_schema": {"type": "function", "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }}
                })
            })
            .collect();
        json!({
            "type": "config",
            "voice_id": self.voice,
            "system_prompt": self.instructions,
            "input_format": "pcm_16000",
            "output_format": "pcm_44100",
            "default_language": self.language,
            "audio_speed": 1.0,
            "push_to_talk": false,
            "stream_ahead_of_real_time": true,
            "tools": tools,
        })
        .to_string()
    }

    fn audio(&self, samples: &[f32]) -> Vec<String> {
        samples
            .chunks(FRAME)
            .map(|frame| {
                json!({"type": "audio_chunk", "audio": STANDARD.encode(pcm16(frame))}).to_string()
            })
            .collect()
    }

    fn tool_result(&self, call: &ToolCall, result: &Value) -> String {
        json!({"type": "tool_call_output", "tool_call_id": call.id, "output": result}).to_string()
    }

    fn still_running(&self, _: &ToolCall) -> Option<String> {
        Some(
            json!({"type": "say", "text": "Sigo trabajando en ello, un momento más.", "interruptible": true})
                .to_string(),
        )
    }

    /// Phonic is still waiting for the answer, so a late one is the answer.
    fn late_result(&self, call: &ToolCall, result: &Value) -> String {
        self.tool_result(call, result)
    }

    fn parse(&self, message: &str) -> Vec<Event> {
        parse(message)
    }

    fn output_rate(&self) -> u32 {
        OUTPUT_RATE
    }
}

pub fn parse(message: &str) -> Vec<Event> {
    let Ok(value) = serde_json::from_str::<Value>(message) else {
        return vec![Event::Error("Phonic sent something unreadable.".into())];
    };
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    match value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "ready_to_start_conversation" | "conversation_created" => vec![Event::Ready],
        "audio_chunk" => STANDARD
            .decode(text("audio"))
            .map(|bytes| vec![Event::Audio(from_pcm16(&bytes))])
            .unwrap_or_default(),
        "input_text" => vec![Event::Heard(text("text"))],
        "output_text" | "assistant_text" => vec![Event::Said(text("text"))],
        "user_started_speaking" => vec![Event::Interrupted],
        "assistant_finished_speaking" => vec![Event::TurnDone],
        "tool_call" => vec![Event::Tool(ToolCall {
            id: text("tool_call_id"),
            name: text("tool_name"),
            args: value
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| json!({})),
        })],
        "tool_call_interrupted" => vec![Event::Cancelled(vec![text("tool_call_id")])],
        "error" => {
            let message = value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("Phonic refused the call. Check the key in Settings › Voice.");
            vec![Event::Error(message.to_owned())]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phonic() -> Phonic {
        Phonic::new(
            "ph-test-key",
            "Be brief.",
            vec![Tool {
                name: "ask_agent".into(),
                description: "Ask an agent.".into(),
                parameters: json!({"type": "object"}),
            }],
        )
    }

    #[test]
    fn the_key_is_a_bearer_header() {
        let phonic = phonic();
        assert_eq!(
            phonic.headers(),
            [("Authorization", "Bearer ph-test-key".to_owned())]
        );
        assert!(!phonic.url().contains("ph-test"));
        assert!(!phonic.setup(None).contains("ph-test"));
    }

    #[test]
    fn config_declares_formats_language_and_sync_tools() {
        let config: Value = serde_json::from_str(&phonic().setup(None)).unwrap();
        assert_eq!(config["type"], "config");
        assert_eq!(config["input_format"], "pcm_16000");
        assert_eq!(config["output_format"], "pcm_44100");
        assert_eq!(config["default_language"], "es");
        assert_eq!(config["system_prompt"], "Be brief.");
        assert_eq!(config["tools"][0]["execution_mode"], "sync");
        assert_eq!(
            config["tools"][0]["tool_schema"]["function"]["name"],
            "ask_agent"
        );
    }

    #[test]
    fn events_decode() {
        let audio = STANDARD.encode(pcm16(&[0.1, 0.2, 0.3]));
        assert!(matches!(
            &parse(&json!({"type": "audio_chunk", "audio": audio}).to_string())[0],
            Event::Audio(samples) if samples.len() == 3
        ));
        assert_eq!(
            parse(r#"{"type":"input_text","text":"hola"}"#),
            [Event::Heard("hola".into())]
        );
        assert_eq!(
            parse(r#"{"type":"user_started_speaking"}"#),
            [Event::Interrupted]
        );
        assert_eq!(
            parse(r#"{"type":"assistant_finished_speaking"}"#),
            [Event::TurnDone]
        );
        assert_eq!(
            parse(
                r#"{"type":"tool_call","tool_call_id":"t1","tool_name":"ask_agent","parameters":{"request":"x"}}"#
            ),
            [Event::Tool(ToolCall {
                id: "t1".into(),
                name: "ask_agent".into(),
                args: json!({"request": "x"})
            })]
        );
        assert_eq!(
            parse(r#"{"type":"tool_call_interrupted","tool_call_id":"t1"}"#),
            [Event::Cancelled(vec!["t1".into()])]
        );
        assert!(matches!(parse(r#"{"type":"error"}"#)[0], Event::Error(_)));
        assert!(parse(r#"{"type":"something_new"}"#).is_empty());
    }

    #[test]
    fn a_slow_tool_says_it_is_still_working_and_answers_later() {
        let phonic = phonic();
        let call = ToolCall {
            id: "t1".into(),
            name: "ask_agent".into(),
            args: json!({}),
        };
        let say: Value = serde_json::from_str(&phonic.still_running(&call).unwrap()).unwrap();
        assert_eq!(say["type"], "say");
        let late: Value =
            serde_json::from_str(&phonic.late_result(&call, &json!({"result": "ok"}))).unwrap();
        assert_eq!(late["type"], "tool_call_output");
        assert_eq!(late["tool_call_id"], "t1");
        assert_eq!(late["output"]["result"], "ok");
    }
}
