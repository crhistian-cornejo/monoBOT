//! Runs a benchmark's raw transcripts through the text a dictation ends
//! with (the whole-clip path: sentences, `join`, then the vocabulary), so
//! the cleanup can be measured on real recordings. Ignored by default:
//! `DICTATION_BENCH=<results.json> [DICTATION_VOCABULARY=<vocab.json>]
//! cargo test -p monobot-dictation bench_cleanup -- --ignored --nocapture`
//! writes `<results>.clean.json` beside it.
use crate::text;

#[test]
#[ignore]
fn bench_cleanup() {
    let Ok(path) = std::env::var("DICTATION_BENCH") else {
        return;
    };
    let vocabulary: Vec<text::Replacement> = std::env::var("DICTATION_VOCABULARY")
        .ok()
        .map(|path| serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap())
        .unwrap_or_default();
    let mut results: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for row in results["rows"].as_array_mut().unwrap() {
        let raw = row["text"].as_str().unwrap_or_default().to_owned();
        let clean = text::apply_vocabulary(&text::join(&text::sentences(&raw)), &vocabulary);
        row["clean"] = clean.into();
    }
    let out = path.replace(".json", ".clean.json");
    std::fs::write(&out, serde_json::to_string_pretty(&results).unwrap()).unwrap();
    println!("wrote {out}");
}
