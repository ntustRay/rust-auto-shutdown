/// `/t 0` 避免 Windows 在正數逾時時隱含 `/f`，保留應用程式阻擋關機的機會。
#[cfg(any(windows, test))]
pub const SHUTDOWN_ARGS: [&str; 3] = ["/s", "/t", "0"];

#[cfg(any(target_os = "macos", test))]
const MACOS_SHUTDOWN_ARGS: [&str; 2] = ["-e", r#"tell application "System Events" to shut down"#];

#[cfg(any(target_os = "linux", test))]
const LINUX_SHUTDOWN_ARGS: [&str; 9] = [
    "--system",
    "--allow-interactive-authorization=no",
    "call",
    "org.freedesktop.login1",
    "/org/freedesktop/login1",
    "org.freedesktop.login1.Manager",
    "PowerOff",
    "b",
    "false",
];

#[cfg(windows)]
pub fn execute() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    // 使用系統目錄的明確路徑，不從目前目錄或 PATH 搜尋執行檔。
    let root = std::env::var_os("SystemRoot").ok_or("找不到 Windows 系統目錄。")?;
    let executable = std::path::PathBuf::from(root)
        .join("System32")
        .join("shutdown.exe");
    let output = std::process::Command::new(executable)
        .args(SHUTDOWN_ARGS)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .map_err(|e| format!("無法呼叫 Windows 關機：{e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Windows 未接受關機要求（代碼 {:?}）。請確認關機權限或系統是否已有其他排程。",
            output.status.code()
        ))
    }
}

#[cfg(target_os = "macos")]
pub fn execute() -> Result<(), String> {
    run_system_command(
        "/usr/bin/osascript",
        &MACOS_SHUTDOWN_ARGS,
        "macOS",
        "請在已登入的桌面執行，並確認「系統設定 → 隱私權與安全性 → 自動化」允許控制 System Events。",
    )
}

#[cfg(target_os = "linux")]
pub fn execute() -> Result<(), String> {
    // 由 logind 檢查目前登入使用者的權限與關機抑制鎖。
    run_system_command(
        "/usr/bin/busctl",
        &LINUX_SHUTDOWN_ARGS,
        "Linux",
        "請確認已安裝 busctl、logind 正在運作，且目前登入的使用者有關機權限。",
    )
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn run_system_command(
    executable: &str,
    args: &[&str],
    platform: &str,
    advice: &str,
) -> Result<(), String> {
    let output = std::process::Command::new(executable)
        .args(args)
        .output()
        .map_err(|error| format!("無法呼叫 {platform} 關機：{error}。{advice}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let reason = stderr.trim();
    Err(format!(
        "{platform} 未接受關機要求（{}）。{advice}\n{reason}",
        output.status
    ))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn execute() -> Result<(), String> {
    Err("真正關機功能支援 Windows、macOS 與使用 logind 的 Linux。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_shutdown_command_does_not_force_apps_or_use_os_countdown() {
        assert_eq!(SHUTDOWN_ARGS, ["/s", "/t", "0"]);
        assert!(!SHUTDOWN_ARGS.contains(&"/f"));
    }

    #[test]
    fn linux_shutdown_uses_logind_without_interactive_authorization() {
        assert_eq!(
            &LINUX_SHUTDOWN_ARGS[..3],
            ["--system", "--allow-interactive-authorization=no", "call"]
        );
        assert_eq!(
            &LINUX_SHUTDOWN_ARGS[3..],
            [
                "org.freedesktop.login1",
                "/org/freedesktop/login1",
                "org.freedesktop.login1.Manager",
                "PowerOff",
                "b",
                "false",
            ]
        );
    }

    #[test]
    fn macos_shutdown_targets_the_current_desktop_with_system_events() {
        assert_eq!(
            MACOS_SHUTDOWN_ARGS,
            ["-e", r#"tell application "System Events" to shut down"#]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_shutdown_script_compiles_without_sending_the_event() {
        let compiled = std::env::temp_dir().join(format!(
            "r-auto-shutdown-script-test-{}.scpt",
            std::process::id()
        ));
        let output = std::process::Command::new("/usr/bin/osacompile")
            .arg("-o")
            .arg(&compiled)
            .args(MACOS_SHUTDOWN_ARGS)
            .output()
            .expect("launch osacompile");
        let _ = std::fs::remove_file(compiled);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
