//! Time prompt processing and generation: cargo run --release -p hark-llm --example bench -- <model.gguf> [gpu_layers]
use hark_llm::backend::{ChatMessage, GenOptions, LlmBackend};
use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("model path");
    let layers: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(999);
    let t = Instant::now();
    let engine = hark_llm::LlamaEngine::load(std::path::Path::new(&path), layers, 8192).expect("load");
    println!("load: {:.1}s", t.elapsed().as_secs_f32());
    let filler = "The team discussed the launch date, the Stripe integration, budget, and the QA contractor. ".repeat(60);
    let msgs = [
        ChatMessage::system("You rewrite meeting notes clearly."),
        ChatMessage::user(format!("{filler}\n\nWrite 300 words summarising the above in detail.")),
    ];
    let mut n = 0usize;
    let mut first: Option<f32> = None;
    let t = Instant::now();
    let out = engine
        .chat(&msgs, &GenOptions { max_tokens: 400, temperature: 0.0, json: false }, &mut |_| {
            n += 1;
            if first.is_none() {
                first = Some(t.elapsed().as_secs_f32());
            }
            true
        })
        .expect("chat");
    let total = t.elapsed().as_secs_f32();
    let ff = first.unwrap_or(total);
    println!("prompt ~{} chars, first token after {:.2}s, {} pieces in {:.1}s => {:.1} tok/s", filler.len(), ff, n, total, n as f32 / (total - ff).max(0.01));
    println!("{}", out.chars().take(120).collect::<String>());
}
