fn main() {
    println!("cargo:rerun-if-changed=assets/icons/git-agent.ico");

    #[cfg(target_os = "windows")]
    winresource::WindowsResource::new()
        .set_icon("assets/icons/git-agent.ico")
        .compile()
        .expect("failed to embed the Git Agent Windows icon");
}
