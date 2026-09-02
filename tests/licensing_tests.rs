#[test]
fn license_and_notice_keep_the_combined_terms_and_attribution() {
    let license = include_str!("../LICENSE");
    let notice = include_str!("../NOTICE");
    let manifest = include_str!("../Cargo.toml");

    assert!(manifest.contains("license-file = \"LICENSE\""));
    assert!(!manifest.lines().any(|line| line.starts_with("license =")));
    for text in [license, notice] {
        assert!(text.contains("\"Commons Clause\" License Condition v1.0"));
        assert!(text.contains("the right to Sell the Software"));
        assert!(text.contains("entirely or substantially"));
        assert!(text.contains("Software: Git Agent"));
        assert!(text.contains("License: Apache License, Version 2.0"));
        assert!(text.contains("Licensor: adoin"));
    }
    assert!(license.contains("END OF TERMS AND CONDITIONS"));
    assert!(license.contains("Version 2.0, January 2004"));
    assert!(notice.contains("Copyright 2026 adoin"));
    assert!(notice.contains("https://github.com/adoin/git-Agent"));
}

#[test]
fn documentation_does_not_claim_an_unmodified_apache_license() {
    let readme = include_str!("../README.md");
    let chinese = include_str!("../README.zh-CN.md");
    let contributing = include_str!("../CONTRIBUTING.md");
    for text in [readme, chinese, contributing] {
        assert!(text.contains("Commons Clause License Condition v1.0"));
        assert!(text.contains("[LICENSE](LICENSE)"));
        assert!(text.contains("source-available"));
    }
    assert!(!readme.contains("does not currently include a `LICENSE`"));
    assert!(!chinese.contains("目前不包含 `LICENSE`"));
}

#[test]
fn every_native_package_carries_license_and_notice() {
    let windows = include_str!("../installer/windows/git-agent.iss");
    let macos = include_str!("../installer/macos/package.sh");
    let linux = include_str!("../installer/linux/package-deb.sh");
    for name in ["LICENSE", "NOTICE"] {
        assert!(windows.contains(&format!(
            "Source: \"..\\..\\{name}\"; DestDir: \"{{app}}\"; Flags: ignoreversion"
        )));
        let install = format!("install -m 644 {name} \"$resources_dir/{name}\"");
        assert!(macos.contains(&install));
        assert!(macos.find(&install).unwrap() < macos.find("codesign --force").unwrap());
    }
    assert!(linux.contains("install -m 644 LICENSE \"$package_root/usr/share/doc/git-agent/copyright\""));
    assert!(linux.contains("install -m 644 NOTICE \"$package_root/usr/share/doc/git-agent/NOTICE\""));
}
