//! Schrödinger "Ask" plugin: put a clip to a model running on this machine.
//!
//! The model is whatever Ollama has installed locally. The clip never
//! leaves the machine: requests go to Ollama on 127.0.0.1, and models that
//! Ollama would run on its cloud are never picked.
//!
//! Every format runs when Enter is pressed, shows what the model wrote, and
//! pastes it on a second Enter. Nothing is sent to the model merely because
//! the selection passed over a format.

use schrodinger_plugin_sdk::prelude::*;
use schrodinger_plugin_sdk::schrodinger_plugin;
use std::time::Duration;

pub mod ollama;

#[derive(Default)]
pub struct AskPlugin;

const TEXT_FORMATS: &[&str] = &["public.utf8-plain-text", "NSStringPboardType", "text/plain;charset=utf-8", "text/plain", "UTF8_STRING"];
/// Copied files carry their names as text; that is not the clip's content.
const FILE_FORMATS: &[&str] = &["public.file-url", "NSFilenamesPboardType", "text/uri-list", "CF_HDROP"];
/// About 8,000 tokens. Longer clips are refused, not cut: an answer about
/// half a document is worse than no answer.
const MAX_CLIP_CHARS: usize = 32_000;
/// Under the 120 seconds the manifest allows a run.
const TIMEOUT: Duration = Duration::from_secs(110);

const SYSTEM: &str = "You work on a piece of text the user copied. Reply with the result only: no preamble, no closing remarks, no explanation of what you did, and no quotation marks or code fences around it unless the text itself has them. Write in the language of the text unless told otherwise.";

/// What to ask for each saved prompt.
pub fn instruction(mode: &str) -> Option<&'static str> {
    Some(match mode {
        "summarize" => "Summarize the text in a short paragraph. Keep the facts, names and numbers that matter; leave out the rest.",
        "points" => "List the key points of the text as short bullet points, one per line, each starting with \"- \". At most seven.",
        "fix" => "Correct the spelling, grammar and punctuation of the text. Keep its wording, tone and formatting, and change nothing that is not an error.",
        "shorter" => "Rewrite the text to be about half as long. Keep its meaning, tone and point of view.",
        "plain" => "Rewrite the text in plain language that a general reader understands on first reading. Keep every fact.",
        "explain" => "Explain what the text says or does, briefly, for someone who is not familiar with the subject. If it is code, say what it does.",
        "english" => "Translate the text into English. Keep its formatting.",
        _ => return None,
    })
}

/// The prompt sent to the model.
pub fn prompt(mode: &str, input: Option<&str>, clip: &str) -> Result<String, String> {
    let task = if mode == "ask" {
        let question = input.map(str::trim).filter(|q| !q.is_empty()).ok_or("Type ? and a question")?;
        format!("Answer this about the text, or do this with it: {question}")
    } else {
        instruction(mode).ok_or("Unknown format")?.to_string()
    };
    Ok(format!("{task}\n\nThe text is between the markers.\n\n<<<TEXT\n{clip}\nTEXT>>>"))
}

fn has_any(formats: &[String], names: &[&str]) -> bool {
    formats.iter().any(|f| names.iter().any(|n| f.eq_ignore_ascii_case(n)))
}

fn copied_text(entry: &ClipEntry) -> Option<String> {
    if has_any(&entry.meta.formats, FILE_FORMATS) {
        return None;
    }
    let name = TEXT_FORMATS.iter().find(|n| has_any(&entry.meta.formats, &[n]))?;
    Some(String::from_utf8_lossy(&entry.read_format(name).ok()?).into_owned())
}

fn text_format() -> &'static str {
    if cfg!(target_os = "macos") {
        "public.utf8-plain-text"
    } else {
        "text/plain;charset=utf-8"
    }
}

impl NativePlugin for AskPlugin {
    fn analyze(&self, entry: &ClipEntry) -> Result<AnalysisData, PluginError> {
        let askable = copied_text(entry).map(|t| t.chars().filter(|c| c.is_alphanumeric()).count() >= 2).unwrap_or(false);
        Ok(if askable { AnalysisData::new().set("text", true) } else { AnalysisData::empty() })
    }

    fn applies_to(&self, entry: &ClipEntry) -> bool {
        entry.analysis().has("text")
    }

    fn apply(&self, entry: &ClipEntry, mode: Option<&str>) -> Result<Vec<Output>, PluginError> {
        let clip = copied_text(entry).ok_or_else(|| PluginError::from("This clip has no text"))?;
        let clip = clip.trim();
        let length = clip.chars().count();
        if length > MAX_CLIP_CHARS {
            return Err(PluginError(format!("This clip is too long to put to a local model ({length} characters; the limit is {MAX_CLIP_CHARS}). Copy a part of it.")));
        }
        let prompt = prompt(mode.unwrap_or("summarize"), entry.input(), clip).map_err(PluginError)?;

        let config = entry.config();
        let setting = config.get("model").and_then(|v| v.as_str()).unwrap_or("Automatic");
        let models = ollama::models().map_err(PluginError)?;
        let model = ollama::choose(&models, setting)
            .ok_or_else(|| PluginError::from("Ollama has no model installed on this machine. Get one with `ollama pull llama3.2`."))?;

        let answer = ollama::generate(&model.name, SYSTEM, &prompt, TIMEOUT).map_err(PluginError)?;
        Ok(vec![Output::new(text_format()).data(answer.into_bytes())])
    }
}

schrodinger_plugin!(AskPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_prompts() {
        let p = prompt("fix", None, "teh cat").unwrap();
        assert!(p.starts_with("Correct the spelling"));
        assert!(p.ends_with("<<<TEXT\nteh cat\nTEXT>>>"));
        let q = prompt("ask", Some("  who signed it? "), "…").unwrap();
        assert!(q.starts_with("Answer this about the text, or do this with it: who signed it?"));
        assert!(prompt("ask", None, "x").is_err());
        assert!(prompt("ask", Some("   "), "x").is_err());
        assert!(prompt("nonsense", None, "x").is_err());
    }

    #[test]
    fn every_listed_format_has_an_instruction() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
        for mode in manifest["modes"].as_array().unwrap() {
            let id = mode["id"].as_str().unwrap();
            assert!(id == "ask" || instruction(id).is_some(), "{id}");
        }
        assert_eq!(manifest["prompt"]["mode"], "ask");
    }

    /// Needs Ollama running with a local model:
    ///   cargo test real -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_answers() {
        let models = ollama::models().expect("Ollama reachable");
        let setting = std::env::var("MODEL").unwrap_or_else(|_| "Automatic".into());
        let model = ollama::choose(&models, &setting).expect("a local model");
        println!("model: {} ({:.1} GB)", model.name, model.size as f64 / 1e9);
        assert!(model.is_local());
        let clip = "Hi Sam, the meetting is moved to thursday at 3pm becuase Priya is travelling. Can you bring the Q3 numbers, about 4.2 million, and book room 12? Thanks, Jo";
        for (mode, input) in [("fix", None), ("points", None), ("ask", Some("What time is the meeting, and who is travelling?"))] {
            let started = std::time::Instant::now();
            let answer = ollama::generate(&model.name, SYSTEM, &prompt(mode, input, clip).unwrap(), TIMEOUT).unwrap();
            println!("--- {mode} ({:.1}s)\n{answer}", started.elapsed().as_secs_f64());
        }
    }
}
