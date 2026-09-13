use sha2::{Digest, Sha256};
use std::path::Path;

pub fn hash_workspace_root(path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    format!("{digest:x}")
}
