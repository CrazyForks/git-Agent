#[test]
fn windows_installer_and_executable_use_the_git_agent_icon() {
    let manifest = include_str!("../Cargo.toml");
    let build_script = include_str!("../build.rs");
    let installer = include_str!("../installer/windows/git-agent.iss");
    let icon = include_bytes!("../assets/icons/git-agent.ico");

    assert!(manifest.contains("winresource = \"0.1\""));
    assert!(build_script.contains(".set_icon(\"assets/icons/git-agent.ico\")"));
    assert!(installer.contains("SetupIconFile=..\\..\\assets\\icons\\git-agent.ico"));
    assert!(installer.contains("IconFilename: \"{app}\\git-agent.exe\""));
    assert_eq!(&icon[..6], &[0, 0, 1, 0, 1, 0]);
    assert!(icon.len() > 1_000);
}
