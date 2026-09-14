# Models and hardware

Everything runs on your machine. Hark chooses model sizes from what it finds:

| Tier | Picked when | Live captions | Final transcript | Summaries and chat |
|---|---|---|---|---|
| CPU, light | < 12 GB RAM, no GPU | Whisper base | Whisper small | Qwen3 1.7B |
| CPU, roomy | 12 GB+ RAM, no GPU | Whisper small | Whisper medium | Qwen3 4B |
| GPU | NVIDIA 6 GB+ VRAM or Apple Silicon | Whisper small | Whisper large-v3-turbo | Qwen3 8B |

Speaker identification always uses pyannote segmentation 3.0 (6 MB) and NeMo TitaNet small (40 MB).

Override any of this in Settings > AI models. Bigger models are better but slower; on CPU the 8B model can take a minute or two to summarise a long meeting.

## Using your own model server

If you already run Ollama, LM Studio, or anything with an OpenAI-compatible API, point Hark at it: Settings > AI assistant > "On an endpoint I run". Transcription still runs in Hark; only summaries, cleanup and chat go to your endpoint.

## Where models live

`<data folder>/models`. Delete files there (or from Settings) to free space; Hark re-downloads on demand.

## Adding models

`crates/hark-models/catalog.json` lists every model with its URL, size and SHA-256. Pull requests adding models are welcome: whisper.cpp `ggml-*.bin` files for speech and GGUF files with a chat template for language models.
