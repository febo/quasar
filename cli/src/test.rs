use {
    crate::{config::QuasarConfig, error::CliResult, style},
    std::process::Command,
};

pub fn run(
    debug: bool,
    filter: Option<String>,
    watch: bool,
    no_build: bool,
    features: Option<String>,
) -> CliResult {
    if watch {
        return run_watch(debug, filter, no_build, features);
    }
    run_once(debug, filter.as_deref(), no_build, features.as_deref())
}

fn run_once(
    debug: bool,
    filter: Option<&str>,
    no_build: bool,
    features: Option<&str>,
) -> CliResult {
    let config = QuasarConfig::load()?;

    if !no_build {
        crate::build::run(debug, false, features.map(String::from))?;
    }

    if config.has_typescript_tests() {
        run_typescript_tests(&config, filter)
    } else if config.has_rust_tests() {
        run_rust_tests(&config, filter)
    } else {
        println!("  {}", style::warn("no test framework configured"));
        Ok(())
    }
}

fn run_watch(
    debug: bool,
    filter: Option<String>,
    no_build: bool,
    features: Option<String>,
) -> CliResult {
    if let Err(e) = run_once(debug, filter.as_deref(), no_build, features.as_deref()) {
        eprintln!("  {}", style::fail(&format!("{e}")));
    }

    loop {
        let baseline = crate::build::collect_mtimes(std::path::Path::new("src"));
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let current = crate::build::collect_mtimes(std::path::Path::new("src"));
            if current != baseline {
                if let Err(e) = run_once(debug, filter.as_deref(), no_build, features.as_deref()) {
                    eprintln!("  {}", style::fail(&format!("{e}")));
                }
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TypeScript (vitest)
// ---------------------------------------------------------------------------

fn run_typescript_tests(config: &QuasarConfig, filter: Option<&str>) -> CliResult {
    let ts = config.testing.typescript.as_ref();
    let install_cmd = ts.map(|t| t.install.as_str()).unwrap_or("npm install");
    let test_cmd = ts.map(|t| t.test.as_str()).unwrap_or("npx vitest run");

    if !std::path::Path::new("node_modules").exists() {
        run_shell_cmd(install_cmd)?;
    }

    run_test_cmd(test_cmd, filter)
}

// ---------------------------------------------------------------------------
// Rust (cargo test)
// ---------------------------------------------------------------------------

fn run_rust_tests(config: &QuasarConfig, filter: Option<&str>) -> CliResult {
    let test_cmd = config
        .testing
        .rust
        .as_ref()
        .map(|r| r.test.as_str())
        .unwrap_or("cargo test tests::");

    run_test_cmd(test_cmd, filter)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn run_shell_cmd(cmd_str: &str) -> CliResult {
    let parts: Vec<&str> = cmd_str.split_whitespace().collect();
    let status = Command::new(parts[0]).args(&parts[1..]).status();

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => {
            eprintln!("  {}", style::fail(&format!("{cmd_str} failed")));
            std::process::exit(s.code().unwrap_or(1));
        }
        Err(e) => {
            eprintln!(
                "  {}",
                style::fail(&format!("failed to run {cmd_str}: {e}"))
            );
            std::process::exit(1);
        }
    }
}

fn run_test_cmd(test_cmd: &str, filter: Option<&str>) -> CliResult {
    let parts: Vec<&str> = test_cmd.split_whitespace().collect();
    let mut cmd = Command::new(parts[0]);
    cmd.args(&parts[1..]);

    if let Some(pattern) = filter {
        cmd.args(["-t", pattern]);
    }

    let status = cmd.status();

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!(
                "  {}",
                style::fail(&format!("failed to run {test_cmd}: {e}"))
            );
            std::process::exit(1);
        }
    }
}
