//! Re-runs the build when a migration file changes, so `sqlx::migrate!` in `storage` embeds
//! the current set (SPEC decision 57).

fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
