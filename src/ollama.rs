//! Talking to Ollama on this machine.

use serde::Deserialize;
use std::time::Duration;

pub const BASE: &str = "http://127.0.0.1:11434";

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Model {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub modified_at: String,
    #[serde(default)]
    pub remote_host: Option<String>,
    #[serde(default)]
    pub remote_model: Option<String>,
    #[serde(default)]
    pub details: Details,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct Details {
    #[serde(default)]
    pub family: String,
}

#[derive(Deserialize)]
struct Tags {
    #[serde(default)]
    models: Vec<Model>,
}

impl Model {
    /// Runs on this machine. Ollama also lists "cloud" models, which send
    /// the prompt to Ollama's servers; those are never used here.
    pub fn is_local(&self) -> bool {
        self.remote_host.is_none() && self.remote_model.is_none() && self.size > 0 && !self.name.ends_with(":cloud") && !self.name.ends_with("-cloud")
    }

    /// A general model to talk to, as opposed to one for embeddings, OCR,
    /// code completion or tool calling.
    pub fn is_general(&self) -> bool {
        let name = self.name.to_ascii_lowercase();
        let family = self.details.family.to_ascii_lowercase();
        const NOT: &[&str] = &["embed", "bert", "ocr", "code", "sql", "vision", "llava", "internvl", "tool-use", "function", "paraphrase", "minilm", "rerank", "guard"];
        !NOT.iter().any(|w| name.contains(w) || family.contains(w))
    }
}

const GB: u64 = 1_000_000_000;

/// Choose the model to use.
///
/// A name from the settings picks the newest installed local model whose
/// name starts with it. "Automatic" picks a general local model of a size
/// that answers in seconds on a laptop (2 to 12 GB), the most recently
/// pulled one, on the view that it is what the user last cared to install.
pub fn choose<'a>(models: &'a [Model], setting: &str) -> Option<&'a Model> {
    let local: Vec<&Model> = models.iter().filter(|m| m.is_local()).collect();
    let newest = |candidates: Vec<&'a Model>| candidates.into_iter().max_by(|a, b| a.modified_at.cmp(&b.modified_at));
    if setting != "Automatic" && !setting.is_empty() {
        let wanted = setting.to_ascii_lowercase();
        if let Some(m) = newest(local.iter().copied().filter(|m| m.name.to_ascii_lowercase().starts_with(&wanted)).collect()) {
            return Some(m);
        }
    }
    let general: Vec<&Model> = local.iter().copied().filter(|m| m.is_general()).collect();
    newest(general.iter().copied().filter(|m| (2 * GB..=12 * GB).contains(&m.size)).collect())
        .or_else(|| general.iter().copied().min_by_key(|m| m.size))
        .or_else(|| local.iter().copied().min_by_key(|m| m.size))
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_millis(1500)).timeout(timeout).build()
}

fn explain(e: ureq::Error) -> String {
    match e {
        ureq::Error::Transport(t) => {
            let text = t.to_string();
            if text.contains("timed out") || text.contains("timeout") {
                "The model took too long to answer. Try a shorter clip or a smaller model.".to_string()
            } else {
                "Ollama is not running. Open the Ollama app (or run `ollama serve`) and try again.".to_string()
            }
        }
        ureq::Error::Status(code, response) => {
            let body = response.into_string().unwrap_or_default();
            let message = serde_json::from_str::<serde_json::Value>(&body).ok().and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string)).unwrap_or(body);
            format!("Ollama answered with an error ({code}): {}", message.trim())
        }
    }
}

pub fn models() -> Result<Vec<Model>, String> {
    let tags: Tags = agent(Duration::from_secs(4)).get(&format!("{BASE}/api/tags")).call().map_err(explain)?.into_json().map_err(|e| format!("Could not read Ollama's model list: {e}"))?;
    Ok(tags.models)
}

#[derive(Deserialize)]
struct Generated {
    #[serde(default)]
    response: String,
}

/// Models that reason out loud wrap it in <think>…</think>. That is not
/// part of the answer.
pub fn strip_thinking(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</think>") {
            Some(end) => rest = &rest[start + end + "</think>".len()..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// Ask `model` and wait for the whole answer.
pub fn generate(model: &str, system: &str, prompt: &str, timeout: Duration) -> Result<String, String> {
    // Room for the prompt and the answer. Ollama's default window is small
    // and cuts long clips off without saying so.
    let estimated_tokens = (system.len() + prompt.len()) / 3;
    let context = (estimated_tokens + 1536).clamp(4096, 32768);
    let request = |think: Option<bool>| {
        let mut body = serde_json::json!({
            "model": model,
            "system": system,
            "prompt": prompt,
            "stream": false,
            "options": { "temperature": 0.2, "num_ctx": context },
        });
        if let Some(think) = think {
            body["think"] = serde_json::Value::Bool(think);
        }
        agent(timeout).post(&format!("{BASE}/api/generate")).send_json(body)
    };
    // Ask reasoning models not to reason at length; a model that does not
    // know the option is asked again without it.
    let response = match request(Some(false)) {
        Err(ureq::Error::Status(400, _)) => request(None),
        other => other,
    }
    .map_err(explain)?;
    let generated: Generated = response.into_json().map_err(|e| format!("Could not read the model's answer: {e}"))?;
    let answer = strip_thinking(&generated.response);
    if answer.is_empty() {
        return Err("The model returned nothing.".into());
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str, gb: f64, family: &str, modified: &str) -> Model {
        Model { name: name.into(), size: (gb * GB as f64) as u64, modified_at: modified.into(), remote_host: None, remote_model: None, details: Details { family: family.into() } }
    }

    fn installed() -> Vec<Model> {
        let mut cloud = model("glm-5.2:cloud", 0.0, "glm", "2026-11-01");
        cloud.remote_host = Some("https://ollama.com".into());
        vec![
            cloud,
            model("llama3.3:latest", 42.5, "llama", "2026-10-01"),
            model("gemma4:latest", 9.6, "gemma4", "2026-09-01"),
            model("qwen3.5:latest", 6.6, "qwen35", "2026-10-20"),
            model("llama3.2:latest", 2.0, "llama", "2026-03-01"),
            model("paraphrase-multilingual:latest", 0.6, "bert", "2026-11-02"),
            model("codestral:latest", 12.6, "llama", "2026-11-03"),
            model("glm-ocr:latest", 2.2, "glmocr", "2026-11-04"),
            model("llama3.2-vision:latest", 7.9, "mllama", "2026-11-05"),
        ]
    }

    #[test]
    fn automatic_picks_a_local_general_model_of_laptop_size() {
        // Newest of the general ones between 2 and 12 GB: not the cloud
        // model, the 42 GB one, the embedder, the OCR, code or vision ones.
        assert_eq!(choose(&installed(), "Automatic").unwrap().name, "qwen3.5:latest");
    }

    #[test]
    fn cloud_models_are_never_chosen() {
        let mut only_cloud = model("kimi-k2.5:cloud", 0.0, "", "2026-11-01");
        only_cloud.remote_model = Some("kimi-k2.5".into());
        assert!(choose(&[only_cloud.clone()], "Automatic").is_none());
        assert!(choose(&[only_cloud], "kimi").is_none());
        assert!(!model("x:cloud", 1.0, "", "").is_local());
    }

    #[test]
    fn a_named_model_wins_and_a_missing_one_falls_back() {
        assert_eq!(choose(&installed(), "gemma").unwrap().name, "gemma4:latest");
        assert_eq!(choose(&installed(), "llama3.2").unwrap().name, "llama3.2-vision:latest", "newest whose name starts with it");
        assert_eq!(choose(&installed(), "phi").unwrap().name, "qwen3.5:latest");
    }

    #[test]
    fn falls_back_when_nothing_is_the_right_size() {
        let big = vec![model("llama3.3:latest", 42.5, "llama", "1"), model("dolphin-mixtral:latest", 26.4, "llama", "2")];
        assert_eq!(choose(&big, "Automatic").unwrap().name, "dolphin-mixtral:latest", "the smallest general one");
        let tools = vec![model("codellama:latest", 3.8, "llama", "1")];
        assert_eq!(choose(&tools, "Automatic").unwrap().name, "codellama:latest", "anything local beats nothing");
        assert!(choose(&[], "Automatic").is_none());
    }

    #[test]
    fn thinking_is_removed() {
        assert_eq!(strip_thinking("<think>\nhmm\n</think>\n\nThe answer."), "The answer.");
        assert_eq!(strip_thinking("Plain answer."), "Plain answer.");
        assert_eq!(strip_thinking("A <think>x</think>B <think>y</think>C"), "A B C");
        assert_eq!(strip_thinking("<think>never closed"), "");
    }
}
