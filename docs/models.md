# Models and hardware

Everything runs on your machine. Hark chooses model sizes from what it finds:

| Tier | Picked when | Live captions | Final transcript | Summaries and chat |
|---|---|---|---|---|
| CPU, light | < 12 GB RAM, no GPU | Whisper base | Whisper small | Qwen3 1.7B |
| CPU, roomy | 12 GB+ RAM, no GPU | Whisper base | Whisper small | Qwen3 4B |
| GPU | NVIDIA 6 GB+ VRAM (CUDA build) or Apple Silicon | Whisper small | Whisper large-v3-turbo | Qwen3 8B |

On CPU-only machines Hark also measures how fast each speech model really runs and, for long recordings, picks the best downloaded model that finishes within the recording's own length (a 90-minute call never becomes a three-hour wait). Whisper medium stays available in Settings for short clips where accuracy matters more than time. Speaker detection runs in parallel slices across your cores and alongside transcription.

The plain Windows (CPU) installer never picks the GPU row, even on a machine with an NVIDIA card: it cannot use the card, and the GPU-sized models would crawl on the CPU. Install the CUDA build to use the card, or pick a tier by hand in Settings.

Speaker identification always uses pyannote segmentation 3.0 (6 MB) and NeMo TitaNet small (40 MB), on the CPU. Long recordings are split into slices that run as parallel processes (up to 8 on a 16-core machine), and clusters with almost no speech are folded into the nearest voice with the count capped at eight. Semantic search in Ask Hark uses BGE small v1.5 (37 MB); without it, search is keyword-only.

Live captions and live notes during a call use the tier's live-caption model and the language model; they are provisional and replaced by the full pass when you stop.

Measured on the CUDA build with an RTX 3080: an 88-minute call is transcribed in under 4 minutes while speakers are detected in parallel, then cleaned up and summarised in about 7 minutes. The language model generates around 90 tokens per second there versus about 10 on the same machine's CPU.

On first run Hark suggests downloading the *essentials* (live-caption model, speaker models, search model) and finishing the larger transcript and language models in the background. Meetings recorded in the meantime get their clean-up and summary automatically once the language model lands.

Override any of this in Settings > AI models. Bigger models are better but slower; on CPU the 8B model can take several minutes to summarise a long meeting, and whisper medium runs at roughly two to three times real time on a fast desktop CPU (a 90-minute call would take hours), which is why the CPU tiers default to small.

## Using your own model server

If you already run Ollama, LM Studio, or anything with an OpenAI-compatible API, point Hark at it: Settings > AI assistant > "On an endpoint I run". Transcription still runs in Hark; only summaries, cleanup and chat go to your endpoint.

## Where models live

`<data folder>/models`. Delete files there (or from Settings) to free space; Hark re-downloads on demand.

## Adding models

`crates/hark-models/catalog.json` lists every model with its URL, size and SHA-256. Pull requests adding models are welcome: whisper.cpp `ggml-*.bin` files for speech and GGUF files with a chat template for language models.
