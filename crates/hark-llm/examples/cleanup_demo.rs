use std::path::Path;
fn main() {
    let p = std::env::var("HARK_TEST_LLM").expect("HARK_TEST_LLM");
    let e = hark_llm::LlamaEngine::load(Path::new(&p), 999, 8192).unwrap();
    let segs: Vec<hark_llm::cleanup::RawSegment> = vec![
        (1, Some("Me".into()), "so um the the launch date is is gonna be uh march third i think yeah".into()),
        (2, Some("Priya".into()), "i am not sure the the client he want more feature before we can to ship".into()),
        (3, Some("Me".into()), "you know what i mean it's uh [inaudible] the conversion rate drop to two percent".into()),
        (4, Some("Priya".into()), "yes agree let's we will revisit on on friday".into()),
    ];
    let t0 = std::time::Instant::now();
    let out = hark_llm::cleanup::run(&e, &segs, |_| {}, || false).unwrap();
    for (id, t) in &out { println!("[{id}] {t}"); }
    println!("({:?}, {} of {} lines)", t0.elapsed(), out.len(), segs.len());
}
