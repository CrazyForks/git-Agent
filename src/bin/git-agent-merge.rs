#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> eframe::Result<()> {
    install_panic_logger();
    git_agent::diagnostics::merge_tool_info(
        "process.start",
        &format!(
            "pid={} exe={} cwd={} args={:?}",
            std::process::id(),
            std::env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|error| format!("<current_exe error: {error}>")),
            std::env::current_dir()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|error| format!("<current_dir error: {error}>")),
            std::env::args().skip(1).collect::<Vec<_>>(),
        ),
    );
    let result = git_agent::merge_tool::MergeToolApp::run_from_env();
    match &result {
        Ok(()) => git_agent::diagnostics::merge_tool_info("process.exit", "outcome=ok"),
        Err(error) => {
            git_agent::diagnostics::merge_tool_error("process.exit", &format!("error={error}"))
        }
    }
    result
}

fn install_panic_logger() {
    std::panic::set_hook(Box::new(|info| {
        git_agent::diagnostics::merge_tool_error(
            "panic",
            &format!(
                "{info} backtrace={}",
                std::backtrace::Backtrace::force_capture()
            ),
        );
    }));
}
