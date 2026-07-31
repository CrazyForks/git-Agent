#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> eframe::Result<()> {
    install_panic_logger();
    append_merge_log(format!(
        "merge tool start pid={} exe={} cwd={} args={:?}",
        std::process::id(),
        std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|error| format!("<current_exe error: {error}>")),
        std::env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|error| format!("<current_dir error: {error}>")),
        std::env::args().skip(1).collect::<Vec<_>>(),
    ));
    let result = git_agent::merge_tool::MergeToolApp::run_from_env();
    append_merge_log(format!(
        "merge tool run_native returned {}",
        match &result {
            Ok(()) => "ok".to_owned(),
            Err(error) => format!("error: {error}"),
        }
    ));
    result
}

fn install_panic_logger() {
    std::panic::set_hook(Box::new(|info| {
        append_merge_log(format!(
            "merge tool panic: {info}\n{}",
            std::backtrace::Backtrace::force_capture()
        ));
    }));
}

fn append_merge_log(message: impl AsRef<str>) {
    let Some(path) = merge_log_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let line = format!("[{}] {}\n", unix_timestamp_millis(), message.as_ref());
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
}

fn merge_log_path() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::PathBuf::from))
        .or_else(|| std::env::current_dir().ok())
        .map(|base| base.join("data").join("merge-tool.log"))
}

fn unix_timestamp_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}
