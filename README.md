# Ask Plugin for Schrödinger

Put a clip to a model running on your own machine, through [Ollama](https://ollama.com).

- **Summarize**, **Key points**, **Fix spelling and grammar**, **Make shorter**, **Plain language**, **Explain**, **Translate to English**
- **Your question**: type `?` in the picker's search field, then anything: `?what is the deadline`, `?reply politely, declining`, `?turn this into a table`

## How it works in the picker

Nothing is sent to the model because the selection passed over a format. Press ↩ on a format to run it; the picker shows that it is working, then shows what the model wrote. Press ↩ again to use it (copy, or ⌘↩ to paste, as with any format). Moving to another format or clip leaves the answer behind.

## What you need

Ollama, running, with at least one model installed (`ollama pull llama3.2` is a small one to start with). If Ollama is not running, the format says so.

## Which model

With **Automatic**, the plugin uses the most recently installed general-purpose model between 2 and 12 GB, on the view that it is the one you last cared to install and that it answers in seconds on a laptop. Models for embeddings, OCR, code completion and tool calling are passed over. Choosing a name in the settings picks the newest installed model whose name starts with it.

**Models that Ollama runs on its cloud are never used**, whatever the setting. Requests go to Ollama on `127.0.0.1`, and the clip stays on your machine.

## Limits

- The first run after a while is slow, because Ollama has to load the model (ten to twenty seconds here); after that, a few seconds.
- A run has two minutes. A clip over 32,000 characters is refused, not cut.
- Small models overreach. Asked to fix spelling, one also changed "travelling" to "traveling" and added a dollar sign to an amount. Read the result before using it; that is what the two-step ↩ is for.
- The answer arrives all at once, not word by word.
- Text clips only.
- The model list in the settings is fixed; a model with another name can only be reached through Automatic.

## Building

```sh
cargo test
cargo test real -- --ignored --nocapture     # against your Ollama; MODEL=gemma to pick one
cargo build --release
```

Install by copying `plugin.json` and the built library (as `lib/libschrodinger_plugin_ask.dylib`) into `~/.schrodinger/plugins/ask/`.

Needs plugin SDK 0.3.0 and a Schrödinger with prompt modes and run-then-paste formats (`"confirm": true`).

## License

MIT
