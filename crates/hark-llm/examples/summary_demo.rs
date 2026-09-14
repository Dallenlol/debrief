use hark_llm::summary::{builtin_templates, generate, transcript_line, TemplateVars};
use std::path::Path;
fn main() {
    let p = std::env::var("HARK_TEST_LLM").expect("HARK_TEST_LLM");
    let e = hark_llm::LlamaEngine::load(Path::new(&p), 999, 8192).unwrap();
    let lines = [
        (0, "Alex", "Okay so the main thing today is the launch date for the new site."),
        (12_000, "Priya", "Right. Design is done, but the checkout integration slipped a week. I think March 3rd is realistic."),
        (30_000, "Alex", "Fine, March 3rd it is. Can you own the Stripe piece and have it in staging by Friday?"),
        (41_000, "Priya", "Yes. One question though, do we still want the annual plan at launch or push it?"),
        (55_000, "Alex", "Push it. Monthly only for launch. I'll tell Sam to update the pricing page."),
        (68_000, "Priya", "Got it. Also the budget: we're at 42k of the 50k, so we have room for the QA contractor."),
        (80_000, "Alex", "Approved, bring them in for two weeks."),
    ];
    let transcript = lines.iter().map(|(ms, sp, t)| transcript_line(*ms, Some(sp), t)).collect::<Vec<_>>().join("\n");
    let vars = TemplateVars { title: "Launch planning".into(), date: "Sep 13, 2026".into(), duration: "1:30".into(), participants: "Alex, Priya".into(), transcript, highlights: String::new() };
    let tpl = builtin_templates().into_iter().find(|t| t.id == "general").unwrap();
    let t0 = std::time::Instant::now();
    let r = generate(&e, &tpl.body, &vars, 60_000).unwrap();
    println!("{}\n--- structured ({:?}) ---\n{}", r.markdown, t0.elapsed(), serde_json::to_string_pretty(&r.structured).unwrap());
}
