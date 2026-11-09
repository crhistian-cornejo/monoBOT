//! Gemini Live: Google's `BidiGenerateContent` WebSocket with a Google AI
//! Studio key in `x-goog-api-key`. Audio goes up as 16 kHz PCM in 20 ms
//! frames and comes back at 24 kHz. Tools are declared non-blocking, so the
//! model keeps talking while an agent works, and a slow result arrives later
//! as a note the model reads when it is idle.
use crate::{Event, Protocol, ToolCall, from_pcm16, pcm16};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

pub const URL: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";
pub const OUTPUT_RATE: u32 = 24_000;
/// The live models IVY tries, best first; a refused model moves to the next.
pub const MODELS: [&str; 3] = [
    "models/gemini-3.8-live",
    "models/gemini-3.1-flash-live-preview",
    "models/gemini-2.5-flash-native-audio-preview-12-2025",
];
/// Gemini's prebuilt voices.
pub const VOICES: [&str; 5] = ["Puck", "Charon", "Kore", "Fenrir", "Aoede"];
/// 20 ms at 16 kHz.
const FRAME: usize = 320;

/// A tool the model may call: its name, what it is for, and its JSON schema.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

pub struct Gemini {
    key: String,
    pub model: String,
    pub voice: String,
    pub instructions: String,
    pub tools: Vec<Tool>,
}

impl Gemini {
    pub fn new(key: &str, instructions: &str, tools: Vec<Tool>) -> Self {
        Self {
            key: key.to_owned(),
            model: MODELS[0].to_owned(),
            voice: VOICES[0].to_owned(),
            instructions: instructions.to_owned(),
            tools,
        }
    }

    /// The model after `model` in the ladder, if any.
    pub fn next_model(model: &str) -> Option<&'static str> {
        let at = MODELS.iter().position(|known| *known == model)?;
        MODELS.get(at + 1).copied()
    }
}

impl Protocol for Gemini {
    fn url(&self) -> &str {
        URL
    }

    fn headers(&self) -> Vec<(&'static str, String)> {
        vec![("x-goog-api-key", self.key.clone())]
    }

    fn setup(&self, handle: Option<&str>) -> String {
        let declarations: Vec<Value> = self
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                    "behavior": "NON_BLOCKING",
                })
            })
            .collect();
        let mut resumption = json!({});
        if let Some(handle) = handle {
            resumption["handle"] = json!(handle);
        }
        let mut setup = json!({
            "model": self.model,
            "generationConfig": {
                "responseModalities": ["AUDIO"],
                "speechConfig": {
                    "voiceConfig": {"prebuiltVoiceConfig": {"voiceName": self.voice}}
                }
            },
            "systemInstruction": {"parts": [{"text": self.instructions}]},
            "inputAudioTranscription": {},
            "outputAudioTranscription": {},
            "sessionResumption": resumption,
            "contextWindowCompression": {"slidingWindow": {}},
        });
        if !declarations.is_empty() {
            setup["tools"] = json!([{"functionDeclarations": declarations}]);
        }
        json!({ "setup": setup }).to_string()
    }

    fn audio(&self, samples: &[f32]) -> Vec<String> {
        samples
            .chunks(FRAME)
            .map(|frame| {
                json!({
                    "realtimeInput": {
                        "audio": {
                            "data": STANDARD.encode(pcm16(frame)),
                            "mimeType": "audio/pcm;rate=16000",
                        }
                    }
                })
                .to_string()
            })
            .collect()
    }

    fn tool_result(&self, call: &ToolCall, result: &Value) -> String {
        response(call, result, "INTERRUPT")
    }

    fn still_running(&self, call: &ToolCall) -> Option<String> {
        Some(response(
            call,
            &json!({"status": "running", "note": "Still working; the result comes later. Tell the user it is on its way and keep talking."}),
            "SILENT",
        ))
    }

    fn late_result(&self, call: &ToolCall, result: &Value) -> String {
        json!({
            "clientContent": {
                "turns": [{
                    "role": "user",
                    "parts": [{"text": format!(
                        "[The {} you asked for earlier has finished. Tell the user what it found.]\n{}",
                        call.name, result
                    )}]
                }],
                "turnComplete": true,
            }
        })
        .to_string()
    }

    fn parse(&self, message: &str) -> Vec<Event> {
        parse(message)
    }

    fn output_rate(&self) -> u32 {
        OUTPUT_RATE
    }
}

fn response(call: &ToolCall, result: &Value, scheduling: &str) -> String {
    json!({
        "toolResponse": {
            "functionResponses": [{
                "id": call.id,
                "name": call.name,
                "response": result,
                "scheduling": scheduling,
            }]
        }
    })
    .to_string()
}

/// Decodes one server message.
pub fn parse(message: &str) -> Vec<Event> {
    let Ok(value) = serde_json::from_str::<Value>(message) else {
        return vec![Event::Error(
            "Gemini Live sent something unreadable.".into(),
        )];
    };
    let mut events = Vec::new();
    if value.get("setupComplete").is_some() {
        events.push(Event::Ready);
    }
    if let Some(content) = value.get("serverContent") {
        if let Some(parts) = content
            .pointer("/modelTurn/parts")
            .and_then(Value::as_array)
        {
            for part in parts {
                if let Some(data) = part.pointer("/inlineData/data").and_then(Value::as_str)
                    && let Ok(bytes) = STANDARD.decode(data)
                {
                    events.push(Event::Audio(from_pcm16(&bytes)));
                }
            }
        }
        if let Some(text) = content
            .pointer("/inputTranscription/text")
            .and_then(Value::as_str)
        {
            events.push(Event::Heard(text.to_owned()));
        }
        if let Some(text) = content
            .pointer("/outputTranscription/text")
            .and_then(Value::as_str)
        {
            events.push(Event::Said(text.to_owned()));
        }
        if content.get("interrupted").and_then(Value::as_bool) == Some(true) {
            events.push(Event::Interrupted);
        }
        if content.get("turnComplete").and_then(Value::as_bool) == Some(true) {
            events.push(Event::TurnDone);
        }
    }
    if let Some(calls) = value
        .pointer("/toolCall/functionCalls")
        .and_then(Value::as_array)
    {
        for call in calls {
            events.push(Event::Tool(ToolCall {
                id: call
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
                name: call
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
                args: call.get("args").cloned().unwrap_or_else(|| json!({})),
            }));
        }
    }
    if let Some(ids) = value
        .pointer("/toolCallCancellation/ids")
        .and_then(Value::as_array)
    {
        events.push(Event::Cancelled(
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        ));
    }
    if value.get("goAway").is_some() {
        events.push(Event::GoAway);
    }
    if let Some(update) = value.get("sessionResumptionUpdate")
        && update.get("resumable").and_then(Value::as_bool) != Some(false)
        && let Some(handle) = update.get("newHandle").and_then(Value::as_str)
    {
        events.push(Event::Resumable(handle.to_owned()));
    }
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Gemini Live refused the call.");
        events.push(Event::Error(message.to_owned()));
    }
    events
}

/// Why a closed connection closed, from its close frame, in the user's words.
pub fn close_reason(code: u16, reason: &str) -> String {
    let lower = reason.to_lowercase();
    if lower.contains("api key")
        || lower.contains("permission")
        || code == 1008 && lower.contains("key")
    {
        "Gemini refused the API key. Check it in Settings › Voice.".into()
    } else if lower.contains("quota") || lower.contains("exhausted") {
        "Gemini's quota for this key is used up for now.".into()
    } else if lower.contains("not found") || lower.contains("not supported") {
        "This Gemini Live model is not available to this key.".into()
    } else if reason.is_empty() {
        "Gemini Live closed the call.".into()
    } else {
        format!("Gemini Live closed the call: {reason}")
    }
}

/// Whether a close says the model is not available, so the next one in
/// `MODELS` may work.
pub fn model_refused(reason: &str) -> bool {
    let lower = reason.to_lowercase();
    lower.contains("not found") || lower.contains("not supported") || lower.contains("quota")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gemini() -> Gemini {
        Gemini::new(
            "AIzaSy-test",
            "You are on a call.",
            vec![Tool {
                name: "ask_agent".into(),
                description: "Ask an agent.".into(),
                parameters: json!({"type": "object", "properties": {"request": {"type": "string"}}, "required": ["request"]}),
            }],
        )
    }

    #[test]
    fn the_key_goes_in_a_header_never_the_url() {
        let gemini = gemini();
        assert!(!gemini.url().contains("AIza"));
        assert_eq!(
            gemini.headers(),
            [("x-goog-api-key", "AIzaSy-test".to_owned())]
        );
        assert!(!gemini.setup(None).contains("AIza"));
    }

    #[test]
    fn setup_asks_for_audio_transcripts_resumption_and_non_blocking_tools() {
        let setup: Value = serde_json::from_str(&gemini().setup(None)).unwrap();
        let setup = &setup["setup"];
        assert_eq!(setup["model"], MODELS[0]);
        assert_eq!(
            setup["generationConfig"]["responseModalities"],
            json!(["AUDIO"])
        );
        assert_eq!(
            setup["generationConfig"]["speechConfig"]["voiceConfig"]["prebuiltVoiceConfig"]["voiceName"],
            "Puck"
        );
        assert_eq!(
            setup["systemInstruction"]["parts"][0]["text"],
            "You are on a call."
        );
        assert!(setup["inputAudioTranscription"].is_object());
        assert!(setup["outputAudioTranscription"].is_object());
        assert!(setup["contextWindowCompression"]["slidingWindow"].is_object());
        assert_eq!(setup["sessionResumption"], json!({}));
        let tool = &setup["tools"][0]["functionDeclarations"][0];
        assert_eq!(tool["name"], "ask_agent");
        assert_eq!(tool["behavior"], "NON_BLOCKING");
        let resumed: Value = serde_json::from_str(&gemini().setup(Some("h-1"))).unwrap();
        assert_eq!(resumed["setup"]["sessionResumption"]["handle"], "h-1");
    }

    #[test]
    fn audio_goes_up_in_twenty_millisecond_frames() {
        let messages = gemini().audio(&vec![0.25; 700]);
        assert_eq!(messages.len(), 3, "320 + 320 + 60 samples");
        let first: Value = serde_json::from_str(&messages[0]).unwrap();
        assert_eq!(
            first["realtimeInput"]["audio"]["mimeType"],
            "audio/pcm;rate=16000"
        );
        let bytes = STANDARD
            .decode(first["realtimeInput"]["audio"]["data"].as_str().unwrap())
            .unwrap();
        assert_eq!(bytes.len(), 640);
    }

    #[test]
    fn server_messages_decode_to_events() {
        let audio = STANDARD.encode(pcm16(&[0.5, -0.5]));
        let message = json!({"serverContent": {
            "modelTurn": {"parts": [{"inlineData": {"mimeType": "audio/pcm;rate=24000", "data": audio}}]},
            "inputTranscription": {"text": "hola"},
            "outputTranscription": {"text": "Hola, ¿qué tal?"},
            "turnComplete": true
        }})
        .to_string();
        let events = parse(&message);
        assert!(matches!(&events[0], Event::Audio(samples) if samples.len() == 2));
        assert_eq!(events[1], Event::Heard("hola".into()));
        assert_eq!(events[2], Event::Said("Hola, ¿qué tal?".into()));
        assert_eq!(events[3], Event::TurnDone);

        assert_eq!(parse(r#"{"setupComplete":{}}"#), [Event::Ready]);
        assert_eq!(
            parse(r#"{"serverContent":{"interrupted":true}}"#),
            [Event::Interrupted]
        );
        assert_eq!(
            parse(
                r#"{"toolCall":{"functionCalls":[{"id":"c1","name":"ask_agent","args":{"request":"x"}}]}}"#
            ),
            [Event::Tool(ToolCall {
                id: "c1".into(),
                name: "ask_agent".into(),
                args: json!({"request": "x"})
            })]
        );
        assert_eq!(
            parse(r#"{"toolCallCancellation":{"ids":["c1","c2"]}}"#),
            [Event::Cancelled(vec!["c1".into(), "c2".into()])]
        );
        assert_eq!(parse(r#"{"goAway":{"timeLeft":"10s"}}"#), [Event::GoAway]);
        assert_eq!(
            parse(r#"{"sessionResumptionUpdate":{"newHandle":"h-2","resumable":true}}"#),
            [Event::Resumable("h-2".into())]
        );
        assert!(
            parse(r#"{"sessionResumptionUpdate":{"newHandle":"h-3","resumable":false}}"#)
                .is_empty()
        );
        assert!(matches!(parse("not json")[0], Event::Error(_)));
    }

    #[test]
    fn tool_answers_in_time_late_or_still_running() {
        let gemini = gemini();
        let call = ToolCall {
            id: "c1".into(),
            name: "ask_agent".into(),
            args: json!({}),
        };
        let now: Value =
            serde_json::from_str(&gemini.tool_result(&call, &json!({"answer": "42"}))).unwrap();
        let response = &now["toolResponse"]["functionResponses"][0];
        assert_eq!(response["id"], "c1");
        assert_eq!(response["response"]["answer"], "42");
        let waiting: Value = serde_json::from_str(&gemini.still_running(&call).unwrap()).unwrap();
        assert_eq!(
            waiting["toolResponse"]["functionResponses"][0]["response"]["status"],
            "running"
        );
        assert_eq!(
            waiting["toolResponse"]["functionResponses"][0]["scheduling"],
            "SILENT"
        );
        let late: Value =
            serde_json::from_str(&gemini.late_result(&call, &json!({"answer": "42"}))).unwrap();
        assert_eq!(late["clientContent"]["turnComplete"], true);
        assert!(
            late["clientContent"]["turns"][0]["parts"][0]["text"]
                .as_str()
                .unwrap()
                .contains("42")
        );
    }

    #[test]
    fn the_ladder_steps_down_and_ends() {
        assert_eq!(Gemini::next_model(MODELS[0]), Some(MODELS[1]));
        assert_eq!(Gemini::next_model(MODELS[2]), None);
        assert_eq!(Gemini::next_model("other"), None);
        assert!(model_refused(
            "models/x is not found for API version v1beta"
        ));
        assert!(!model_refused("API key not valid"));
        assert!(close_reason(1008, "API key not valid").contains("Settings › Voice"));
        assert_eq!(close_reason(1000, ""), "Gemini Live closed the call.");
    }
}
