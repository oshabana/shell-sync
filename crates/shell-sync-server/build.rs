use std::path::Path;

fn main() {
    // rust-embed requires the embedded folder to exist at compile time.
    // web-ui/dist is a Vite build output that is gitignored, so it won't
    // be present in fresh clones. Create it if missing so the derive macro
    // succeeds (it will simply embed zero files).
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web-ui/dist");
    if !dist.exists() {
        std::fs::create_dir_all(&dist).expect("failed to create web-ui/dist");
    }
}
