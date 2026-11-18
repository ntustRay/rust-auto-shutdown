/// `/t 0` 避免 Windows 在正數逾時時隱含 `/f`，保留應用程式阻擋關機的機會。
pub const SHUTDOWN_ARGS: [&str; 3] = ["/s", "/t", "0"];

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

#[cfg(not(windows))]
pub fn execute() -> Result<(), String> {
    Err("真正關機功能僅支援 Windows。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_command_does_not_force_apps_or_use_os_countdown() {
        assert_eq!(SHUTDOWN_ARGS, ["/s", "/t", "0"]);
        assert!(!SHUTDOWN_ARGS.contains(&"/f"));
    }
}
