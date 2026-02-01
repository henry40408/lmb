use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-env-changed=GIT_VERSION");

    // Prefer GIT_VERSION env var (used during Docker builds)
    if let Ok(version) = std::env::var("GIT_VERSION") {
        if !version.is_empty() {
            println!("cargo:rustc-env=APP_VERSION={version}");
            return;
        }
    }

    // Fall back to git describe (used during local development)
    let output = Command::new("git")
        .args(["describe", "--always", "--dirty=-modified", "--tags"])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let git_desc = String::from_utf8_lossy(&out.stdout).trim().to_string();
            println!("cargo:rustc-env=APP_VERSION={git_desc}");
        }
        Ok(out) => {
            println!("cargo:rustc-env=APP_VERSION=unknown");
            println!(
                "cargo:warning=git describe failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Err(e) => {
            println!("cargo:rustc-env=APP_VERSION=unknown");
            println!("cargo:warning=failed to run git: {e:?}");
        }
    }
}
