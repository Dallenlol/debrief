fn main() {
    let set = hark_detect::PatternSet::builtin();
    let windows = hark_detect::list_visible_windows();
    for w in &windows {
        println!("{:<28} {}", w.process, w.title);
    }
    println!("--- detected: {:?}", set.match_windows(&windows));
}
