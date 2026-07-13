use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use cmux_diff::{DiffSessionRegistry, RegisteredFile};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "cmux-diff-unregister-red-{}-{suffix}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temp root");
        Self(std::fs::canonicalize(path).expect("canonical temp root"))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn registered_file(path: PathBuf, request_path: &str) -> RegisteredFile {
    RegisteredFile {
        request_path: request_path.to_string(),
        file_path: path,
        mime_type: "text/html".to_string(),
    }
}

#[test]
fn unregister_removes_only_the_exact_token_and_is_idempotent() {
    let root = TempRoot::new();
    let first_path = root.path().join("first.html");
    let second_path = root.path().join("second.html");
    std::fs::write(&first_path, "first").expect("first fixture");
    std::fs::write(&second_path, "second").expect("second fixture");

    let registry = DiffSessionRegistry::new(root.path());
    let now = SystemTime::now();
    let first = "tok-first-abcdef012345";
    let second = "tok-second-abcdef01234";
    registry
        .register(first, vec![registered_file(first_path, "/first.html")], now)
        .expect("register first");
    registry
        .register(
            second,
            vec![registered_file(second_path, "/second.html")],
            now,
        )
        .expect("register second");

    registry.unregister(first);
    assert!(!registry.has_active_session(first, now));
    assert!(registry.has_active_session(second, now));
    assert!(registry
        .registered_file(second, "/second.html", now)
        .is_some());

    registry.unregister(first);
    assert!(!registry.has_active_session(first, now));
    assert!(registry.has_active_session(second, now));
}
