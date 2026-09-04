#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

const MAIN_WINDOW_INITIAL_WIDTH: f32 = 1360.0;
const MAIN_WINDOW_INITIAL_HEIGHT: f32 = 860.0;
const MAIN_WINDOW_MIN_WIDTH: f32 = MAIN_WINDOW_INITIAL_WIDTH - 200.0;
const MAIN_WINDOW_MIN_HEIGHT: f32 = MAIN_WINDOW_MIN_WIDTH * 9.0 / 16.0;

fn main() -> eframe::Result<()> {
    install_panic_logger();
    git_agent::diagnostics::app_info(
        "process.start",
        &format!(
            "pid={} exe={} cwd={}",
            std::process::id(),
            std::env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|error| format!("<current_exe error: {error}>")),
            std::env::current_dir()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|error| format!("<current_dir error: {error}>"))
        ),
    );

    let initial_window_size = git_agent::app::persisted_window_inner_size(
        [MAIN_WINDOW_INITIAL_WIDTH, MAIN_WINDOW_INITIAL_HEIGHT],
        [MAIN_WINDOW_MIN_WIDTH, MAIN_WINDOW_MIN_HEIGHT],
    );
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Git Agent")
            .with_icon(app_icon_data())
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(true)
            .with_inner_size(initial_window_size)
            .with_min_inner_size([MAIN_WINDOW_MIN_WIDTH, MAIN_WINDOW_MIN_HEIGHT]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "Git Agent",
        options,
        Box::new(|cc| {
            prefer_rounded_window_corners(cc);
            Ok(Box::new(git_agent::app::GitAgentApp::new(cc)))
        }),
    );
    match &result {
        Ok(()) => git_agent::diagnostics::app_info("process.exit", "outcome=ok"),
        Err(error) => git_agent::diagnostics::app_error("process.exit", &format!("error={error}")),
    }
    result
}

#[cfg(target_os = "windows")]
fn prefer_rounded_window_corners(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;

    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_ROUND: u32 = 2;

    let Ok(window_handle) = cc.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = window_handle.as_raw() else {
        return;
    };

    let preference = DWMWCP_ROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(
            handle.hwnd.get() as _,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const u32 as _,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn prefer_rounded_window_corners(_: &eframe::CreationContext<'_>) {}

fn install_panic_logger() {
    std::panic::set_hook(Box::new(|info| {
        git_agent::diagnostics::app_error("panic", &info.to_string());
    }));
}

fn app_icon_data() -> eframe::egui::IconData {
    // Use the same vector artwork as the title bar and generated installer icons.
    // Rasterize once at startup, never in the title-bar painting/dragging path.
    let image = egui_extras::image::load_svg_bytes(include_bytes!("../assets/icons/logo-ga.svg"))
        .expect("embedded Git Agent logo must be valid SVG");
    eframe::egui::IconData {
        rgba: image.pixels.iter().flat_map(|pixel| pixel.to_srgba_unmultiplied()).collect(),
        width: image.size[0] as u32,
        height: image.size[1] as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_icon_has_custom_git_mark() {
        let icon = app_icon_data();
        assert_eq!(icon.width, 64);
        assert_eq!(icon.height, 64);
        assert!(
            icon.rgba
                .chunks_exact(4)
                .any(|px| px == [21, 196, 151, 255])
        );
        assert!(
            icon.rgba
                .chunks_exact(4)
                .any(|px| px == [47, 111, 234, 255])
        );
        assert_eq!(&icon.rgba[0..4], &[0, 0, 0, 0]);
        let logo = include_str!("../assets/icons/logo-ga.svg");
        assert!(!logo.contains("fill=\"#FFFFFF\""));
        assert!(!logo.contains("fill-opacity"));
        assert!(!logo.contains("<rect"));
        assert!(logo.contains("stroke=\"#15C497\""));
        assert!(logo.contains("stroke=\"#2F6FEA\""));
        assert!(logo.contains("<circle cx=\"42\" cy=\"22\""));
        assert!(logo.contains("fill=\"none\""));
        assert!(include_str!("main.rs").contains("with_decorations(false)"));
        assert!(include_str!("main.rs").contains("DWMWA_WINDOW_CORNER_PREFERENCE"));
        assert!(include_str!("main.rs").contains("DWMWCP_ROUND"));
    }

    #[test]
    fn logo_nodes_are_hollow_and_branches_form_one_connected_mark() {
        let icon = app_icon_data();
        let size = icon.width as usize;
        let alpha = |x: usize, y: usize| icon.rgba[(y * size + x) * 4 + 3];
        for (x, y) in [(23, 17), (23, 47), (42, 22)] {
            // A real round hole, not the narrow slit left by a line through its center.
            for dy in -2isize..=1 {
                for dx in -2isize..=1 {
                    assert_eq!(alpha((x as isize + dx) as usize, (y as isize + dy) as usize), 0);
                }
            }
        }
        for (x, y) in [(23, 24), (23, 30), (23, 34), (30, 29), (36, 25), (23, 41)] {
            assert!(alpha(x, y) >= 200, "Gap at {x},{y}");
        }
        let mut visited = std::collections::HashSet::new();
        let mut pending = vec![(23usize, 31usize)];
        while let Some((x, y)) = pending.pop() {
            if alpha(x, y) < 128 || !visited.insert((x, y)) { continue; }
            for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
                if nx < size && ny < size { pending.push((nx, ny)); }
            }
        }
        assert_eq!(visited.len(), icon.rgba.chunks_exact(4).filter(|p| p[3] >= 128).count());
        assert_eq!(include_bytes!("../assets/icons/logo-ga.svg"), include_bytes!("../website/public/assets/logo-ga.svg"));
        assert_eq!(include_bytes!("../assets/icons/logo-ga.png"), include_bytes!("../website/public/assets/logo-ga.png"));
    }

    #[test]
    fn main_installs_file_logging_for_startup_and_panics() {
        let source = include_str!("main.rs");

        assert!(source.contains("diagnostics::app_info("));
        assert!(source.contains("diagnostics::app_error("));
        assert!(source.contains("std::panic::set_hook"));
        assert!(source.contains("\"process.start\""));
        assert!(source.contains("\"panic\""));
        assert!(source.contains("\"process.exit\""));
    }

    #[test]
    fn main_window_minimum_size_uses_width_minus_two_hundred_and_sixteen_by_nine_height() {
        assert_eq!(MAIN_WINDOW_MIN_WIDTH, MAIN_WINDOW_INITIAL_WIDTH - 200.0);
        assert_eq!(MAIN_WINDOW_MIN_HEIGHT, MAIN_WINDOW_MIN_WIDTH * 9.0 / 16.0);
        assert_eq!(MAIN_WINDOW_MIN_WIDTH, 1160.0);
        assert_eq!(MAIN_WINDOW_MIN_HEIGHT, 652.5);
        let source = include_str!("main.rs");
        assert!(
            source.contains("with_min_inner_size([MAIN_WINDOW_MIN_WIDTH, MAIN_WINDOW_MIN_HEIGHT])")
        );
        assert!(source.contains("with_resizable(true)"));
        assert!(source.contains("persisted_window_inner_size("));
    }
}
