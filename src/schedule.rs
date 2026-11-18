//! 每週排程使用 Task Scheduler 的 InteractiveToken，不要求或儲存 Windows 密碼。
//! XML schema: https://learn.microsoft.com/en-us/windows/win32/taskschd/weekly-trigger-example--xml-

use chrono::{DateTime, Datelike, Days, Local, LocalResult, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DAY_LABELS: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];
#[cfg(any(windows, test))]
const XML_DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeeklySchedule {
    /// 星期一至星期日。
    pub days: [bool; 7],
    pub time: String,
    pub simulation: bool,
}

impl Default for WeeklySchedule {
    fn default() -> Self {
        Self {
            days: [true; 7],
            time: "23:30".into(),
            simulation: true,
        }
    }
}

impl WeeklySchedule {
    pub fn validate(&self) -> Result<(), String> {
        if !self.days.iter().any(|selected| *selected) {
            return Err("請至少選擇一個星期。".into());
        }
        parse_clock(&self.time)?;
        Ok(())
    }

    pub fn next_occurrence(&self, now: &DateTime<Local>) -> Result<DateTime<Local>, String> {
        self.next_in_zone(now)
    }

    fn next_in_zone<T: TimeZone>(&self, now: &DateTime<T>) -> Result<DateTime<T>, String> {
        self.validate()?;
        let time = parse_clock(&self.time)?;
        for offset in 0..=7 {
            let date = now
                .date_naive()
                .checked_add_days(Days::new(offset))
                .ok_or("日期超出範圍。")?;
            if !self.days[date.weekday().num_days_from_monday() as usize] {
                continue;
            }
            match now.timezone().from_local_datetime(&date.and_time(time)) {
                LocalResult::Single(target) if target > *now => return Ok(target),
                LocalResult::Single(_) => {}
                _ => return Err("下一次時刻受到時區切換影響，請選擇其他時間。".into()),
            }
        }
        Err("找不到下一次執行時刻。".into())
    }

    pub fn summary(&self) -> String {
        let days = if self.days.iter().all(|day| *day) {
            "每天".into()
        } else if self.days == [true, true, true, true, true, false, false] {
            "每週一至五".into()
        } else if self.days == [false, false, false, false, false, true, true] {
            "每週六、日".into()
        } else {
            format!(
                "每週{}",
                self.days
                    .iter()
                    .zip(DAY_LABELS)
                    .filter_map(|(selected, day)| selected.then_some(day))
                    .collect::<Vec<_>>()
                    .join("、")
            )
        };
        format!("{days} {}", self.time)
    }
}

fn parse_clock(value: &str) -> Result<NaiveTime, String> {
    let bytes = value.as_bytes();
    if bytes.len() != 5
        || bytes[2] != b':'
        || ![bytes[0], bytes[1], bytes[3], bytes[4]]
            .iter()
            .all(u8::is_ascii_digit)
    {
        return Err("請使用 24 小時制 HH:MM，例如 23:30。".into());
    }
    NaiveTime::parse_from_str(value, "%H:%M").map_err(|_| "時間必須介於 00:00 至 23:59。".into())
}

pub fn data_directory() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|base| PathBuf::from(base).join("RustAutoShutdown"))
        .ok_or_else(|| "找不到 LOCALAPPDATA，無法保存排程。".into())
}

fn config_path() -> Result<PathBuf, String> {
    Ok(data_directory()?.join("schedule.json"))
}

/// 設定只有在 Windows 任務建立成功後才更新；若提交設定失敗，復原舊任務。
#[cfg(windows)]
pub fn save(schedule: &WeeklySchedule) -> Result<(), String> {
    schedule.validate()?;
    let directory = data_directory()?;
    std::fs::create_dir_all(&directory).map_err(|error| format!("無法建立設定目錄：{error}"))?;
    let _lock = FileLock::acquire(directory.join("settings.lock"))?;
    let name = task_name()?;
    let previous_xml = query_task_xml(&name)?;
    let executable = directory.join("app.exe");
    let source = std::env::current_exe().map_err(|error| format!("找不到目前執行檔：{error}"))?;
    let source_path = source
        .canonicalize()
        .map_err(|error| format!("無法讀取目前執行檔路徑：{error}"))?;
    if Some(source_path) != executable.canonicalize().ok() {
        std::fs::copy(&source, &executable)
            .map_err(|error| format!("無法保存排程執行檔：{error}"))?;
    }
    let xml = task_xml(schedule, &executable, &current_sid()?, &Local::now())?;
    let xml_path = directory.join("registration.xml");
    write_xml(&xml_path, &xml)?;
    let staged_config = directory.join("schedule.pending.json");
    let json = serde_json::to_vec_pretty(schedule).map_err(|error| error.to_string())?;
    std::fs::write(&staged_config, json).map_err(|error| format!("無法準備排程設定：{error}"))?;
    let created = register_xml(&name, &xml_path);
    let _ = std::fs::remove_file(&xml_path);
    if let Err(error) = created {
        let _ = std::fs::remove_file(&staged_config);
        return Err(error);
    }
    if let Err(error) = atomic_replace(&staged_config, &config_path()?) {
        let rollback = if let Some(old_xml) = previous_xml {
            write_xml(&xml_path, &old_xml).and_then(|()| register_xml(&name, &xml_path))
        } else {
            delete_task(&name)
        };
        let _ = std::fs::remove_file(&xml_path);
        let _ = std::fs::remove_file(&staged_config);
        return Err(match rollback {
            Ok(()) => format!("無法保存設定，已復原原有排程：{error}"),
            Err(rollback_error) => format!(
                "無法保存設定，Windows 排程復原也失敗：{error}；{rollback_error}。請開啟工作排程器停用 {name}。"
            ),
        });
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn save(_schedule: &WeeklySchedule) -> Result<(), String> {
    Err("每週排程僅支援 Windows。".into())
}

pub fn load() -> Result<Option<WeeklySchedule>, String> {
    let path = config_path()?;
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("無法讀取排程設定：{error}")),
    };
    let schedule: WeeklySchedule =
        serde_json::from_slice(&bytes).map_err(|error| format!("排程設定格式錯誤：{error}"))?;
    schedule.validate()?;
    Ok(Some(schedule))
}

#[cfg(windows)]
pub fn task_exists() -> Result<bool, String> {
    Ok(query_task_xml(&task_name()?)?.is_some())
}

#[cfg(not(windows))]
pub fn task_exists() -> Result<bool, String> {
    Ok(false)
}

#[cfg(windows)]
pub fn remove() -> Result<(), String> {
    let directory = data_directory()?;
    if !directory.exists() {
        return Ok(());
    }
    let _lock = FileLock::acquire(directory.join("settings.lock"))?;
    let name = task_name()?;
    if query_task_xml(&name)?.is_some() {
        delete_task(&name)?;
    }
    match std::fs::remove_file(config_path()?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Windows 排程已停用，但設定檔無法清除：{error}")),
    }
}

#[cfg(not(windows))]
pub fn remove() -> Result<(), String> {
    Err("每週排程僅支援 Windows。".into())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunRecord {
    pub scheduled_for: String,
    pub completed_at: String,
    pub simulation: bool,
    pub success: bool,
    pub message: String,
}

pub fn last_run() -> Result<Option<RunRecord>, String> {
    let path = data_directory()?.join("last-run.json");
    read_run_record(&path)
}

fn read_run_record(path: &Path) -> Result<Option<RunRecord>, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|error| {
            format!(
                "執行紀錄格式錯誤，已停止自動執行。請備份並修復 {}：{error}",
                path.display()
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("無法讀取執行紀錄：{error}")),
    }
}

fn write_run_record(path: &Path, record: &RunRecord) -> Result<(), String> {
    use std::io::Write;

    let pending = path.with_extension("pending.json");
    let bytes = serde_json::to_vec_pretty(record).map_err(|error| error.to_string())?;
    let result = (|| {
        let mut file = std::fs::File::create(&pending).map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        atomic_replace(&pending, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&pending);
    }
    result.map_err(|error| format!("無法完整保存執行紀錄，既有紀錄保持不變：{error}"))
}

/// 此入口只接受已保存且模式相符的排程，不讓命令列選項繞過 UI 確認。
pub fn run_scheduled(simulation: bool) -> Result<(), String> {
    let schedule = load()?.ok_or("沒有已啟用的每週排程。")?;
    if simulation != schedule.simulation {
        return Err("工作模式與已保存設定不符，拒絕執行。".into());
    }
    let now = Local::now();
    let time = parse_clock(&schedule.time)?;
    if !schedule.days[now.weekday().num_days_from_monday() as usize] {
        return Err("今天不在排程的星期範圍內。".into());
    }
    let scheduled = now.date_naive().and_time(time);
    let delay = now
        .naive_local()
        .signed_duration_since(scheduled)
        .num_seconds();
    if !(0..120).contains(&delay) {
        return Err("目前不在排程時刻的兩分鐘執行範圍內，未執行關機。".into());
    }
    let directory = data_directory()?;
    let _lock = FileLock::acquire(directory.join("execution.lock"))?;
    let scheduled_for = scheduled.format("%Y-%m-%d %H:%M").to_string();
    if last_run()?.is_some_and(|previous| previous.scheduled_for == scheduled_for) {
        return Err("這次排程已執行過，未重複執行。".into());
    }
    let mut record = RunRecord {
        scheduled_for,
        completed_at: now.to_rfc3339(),
        simulation,
        success: simulation,
        message: if simulation {
            "模擬完成，未執行關機。"
        } else {
            "準備送出 Windows 關機要求。"
        }
        .into(),
    };
    let path = directory.join("last-run.json");
    let write_record = |record: &RunRecord| write_run_record(&path, record);
    write_record(&record)?;
    if simulation {
        return Ok(());
    }
    match crate::shutdown::execute() {
        Ok(()) => {
            record.completed_at = Local::now().to_rfc3339();
            record.success = true;
            record.message = "Windows 已接受關機要求。".into();
            write_record(&record)
        }
        Err(error) => {
            record.completed_at = Local::now().to_rfc3339();
            record.message = format!("執行失敗：{error}");
            let _ = write_record(&record);
            Err(error)
        }
    }
}

struct FileLock {
    _file: std::fs::File,
    #[cfg(not(windows))]
    path: PathBuf,
}

impl FileLock {
    fn acquire(path: PathBuf) -> Result<Self, String> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // 不共享 handle；程序結束時由 Windows 刪除鎖檔，避免中斷後留下永久鎖。
            options
                .share_mode(0)
                .access_mode(0x4000_0000 | 0x0001_0000)
                .custom_flags(0x0400_0000);
        }
        #[cfg(not(windows))]
        options.create_new(true);
        let file = options
            .open(&path)
            .map_err(|error| format!("無法取得排程設定鎖定（其他視窗可能正在更新）：{error}"))?;
        Ok(Self {
            _file: file,
            #[cfg(not(windows))]
            path,
        })
    }
}

#[cfg(not(windows))]
impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(windows)]
fn system_command(name: &str) -> Result<std::process::Command, String> {
    use std::os::windows::process::CommandExt;
    let root = std::env::var_os("SystemRoot").ok_or("找不到 Windows 系統目錄。")?;
    let mut command = std::process::Command::new(PathBuf::from(root).join("System32").join(name));
    command
        .creation_flags(0x0800_0000)
        .stdin(std::process::Stdio::null());
    Ok(command)
}

#[cfg(windows)]
fn current_sid() -> Result<String, String> {
    let output = system_command("whoami.exe")?
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .map_err(|error| format!("無法取得目前使用者：{error}"))?;
    if !output.status.success() {
        return Err("Windows 未回傳目前使用者識別碼。".into());
    }
    String::from_utf8_lossy(&output.stdout)
        .split(',')
        .map(|part| part.trim().trim_matches('"'))
        .find(|part| {
            part.starts_with("S-1-")
                && part
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b'S' || b == b'-')
        })
        .map(str::to_owned)
        .ok_or_else(|| "無法辨識目前使用者 SID。".into())
}

#[cfg(windows)]
pub fn task_name() -> Result<String, String> {
    Ok(format!("RustAutoShutdown-{}", current_sid()?))
}

#[cfg(windows)]
fn query_task_xml(name: &str) -> Result<Option<String>, String> {
    let output = system_command("schtasks.exe")?
        .args(["/Query", "/TN", name, "/XML", "/HRESULT"])
        .output()
        .map_err(|error| format!("無法查詢 Windows 排程：{error}"))?;
    if output.status.success() {
        let bytes = &output.stdout;
        let xml = if bytes.starts_with(&[0xff, 0xfe]) || bytes.get(1) == Some(&0) {
            let start = usize::from(bytes.starts_with(&[0xff, 0xfe])) * 2;
            let words = bytes[start..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>();
            String::from_utf16(&words).map_err(|_| "Windows 排程 XML 編碼錯誤。")?
        } else {
            String::from_utf8(bytes.clone()).map_err(|_| "Windows 排程 XML 編碼錯誤。")?
        };
        Ok(Some(xml))
    } else if output.status.code().map(|code| code as u32) == Some(0x8007_0002) {
        Ok(None)
    } else {
        Err(format!(
            "無法查詢專用 Windows 排程（代碼 {:?}）。",
            output.status.code()
        ))
    }
}

#[cfg(windows)]
fn register_xml(name: &str, path: &Path) -> Result<(), String> {
    let output = system_command("schtasks.exe")?
        .args(["/Create", "/TN", name, "/XML"])
        .arg(path)
        .args(["/F", "/HRESULT"])
        .output()
        .map_err(|error| format!("無法建立 Windows 排程：{error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Windows 未接受排程（代碼 {:?}）。請確認工作排程器服務與目前使用者的建立工作權限。",
            output.status.code()
        ))
    }
}

#[cfg(windows)]
fn delete_task(name: &str) -> Result<(), String> {
    let output = system_command("schtasks.exe")?
        .args(["/Delete", "/TN", name, "/F", "/HRESULT"])
        .output()
        .map_err(|error| format!("無法停用 Windows 排程：{error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Windows 未能停用專用排程（代碼 {:?}）。",
            output.status.code()
        ))
    }
}

#[cfg(windows)]
fn write_xml(path: &Path, xml: &str) -> Result<(), String> {
    let bytes = std::iter::once(0xfeff_u16)
        .chain(xml.encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    std::fs::write(path, bytes).map_err(|error| format!("無法準備 Windows 排程 XML：{error}"))
}

#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: nul-terminated buffers remain alive for the synchronous Win32 call.
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0x1 | 0x8) } != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::rename(source, target).map_err(|error| error.to_string())
}

#[cfg(any(windows, test))]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(any(windows, test))]
fn task_xml<T: TimeZone>(
    schedule: &WeeklySchedule,
    executable: &Path,
    sid: &str,
    now: &DateTime<T>,
) -> Result<String, String> {
    schedule.validate()?;
    let executable = executable
        .to_str()
        .ok_or("排程執行檔路徑不是有效 Unicode。")?;
    let selected_days = schedule
        .days
        .iter()
        .zip(XML_DAYS)
        .filter(|(selected, _day)| **selected)
        .map(|(_selected, day)| format!("<{day}/>"))
        .collect::<String>();
    let argument = if schedule.simulation {
        "--scheduled-dry-run"
    } else {
        "--scheduled-shutdown"
    };
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Rust Auto Shutdown weekly schedule for the current user.</Description></RegistrationInfo>
  <Triggers><CalendarTrigger><StartBoundary>{date}T{time}:00</StartBoundary><Enabled>true</Enabled><ScheduleByWeek><WeeksInterval>1</WeeksInterval><DaysOfWeek>{selected_days}</DaysOfWeek></ScheduleByWeek></CalendarTrigger></Triggers>
  <Principals><Principal id="CurrentUser"><UserId>{sid}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><AllowHardTerminate>false</AllowHardTerminate><StartWhenAvailable>false</StartWhenAvailable><RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable><AllowStartOnDemand>false</AllowStartOnDemand><Enabled>true</Enabled><Hidden>false</Hidden><RunOnlyIfIdle>false</RunOnlyIfIdle><WakeToRun>false</WakeToRun><ExecutionTimeLimit>PT5M</ExecutionTimeLimit><Priority>7</Priority></Settings>
  <Actions Context="CurrentUser"><Exec><Command>{executable}</Command><Arguments>{argument}</Arguments></Exec></Actions>
</Task>"#,
        date = now.date_naive().format("%Y-%m-%d"),
        time = schedule.time,
        sid = xml_escape(sid),
        executable = xml_escape(executable),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn now() -> DateTime<FixedOffset> {
        FixedOffset::east_opt(8 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 11, 17, 23, 30, 0)
            .unwrap()
    }

    struct ScratchRecord(PathBuf);

    impl ScratchRecord {
        fn new(label: &str) -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "rust-auto-shutdown-{label}-{}-{nonce}",
                std::process::id()
            ));
            std::fs::create_dir(&directory).unwrap();
            Self(directory.join("last-run.json"))
        }
    }

    impl Drop for ScratchRecord {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            let _ = std::fs::remove_file(self.0.with_extension("pending.json"));
            let _ = std::fs::remove_dir(self.0.parent().unwrap());
        }
    }

    #[test]
    fn interrupted_pending_write_and_failed_replace_preserve_previous_record() {
        let scratch = ScratchRecord::new("atomic-record");
        let mut record = RunRecord {
            scheduled_for: "2026-11-18 23:30".into(),
            completed_at: "2026-11-18T23:30:00+08:00".into(),
            simulation: false,
            success: false,
            message: "準備送出 Windows 關機要求。".into(),
        };
        write_run_record(&scratch.0, &record).unwrap();
        std::fs::write(
            scratch.0.with_extension("pending.json"),
            b"{\"scheduled_for\":",
        )
        .unwrap();
        let prior = read_run_record(&scratch.0).unwrap().unwrap();
        assert_eq!(prior.scheduled_for, record.scheduled_for);
        assert_eq!(prior.message, record.message);
        assert!(atomic_replace(&scratch.0.with_extension("missing"), &scratch.0).is_err());
        assert_eq!(
            read_run_record(&scratch.0).unwrap().unwrap().message,
            record.message
        );
        record.success = true;
        record.message = "Windows 已接受關機要求。".into();
        write_run_record(&scratch.0, &record).unwrap();
        let updated = read_run_record(&scratch.0).unwrap().unwrap();
        assert!(updated.success);
        assert_eq!(updated.message, record.message);
        assert!(!scratch.0.with_extension("pending.json").exists());
    }

    #[test]
    fn corrupt_record_is_reported_instead_of_treated_as_missing() {
        let scratch = ScratchRecord::new("corrupt-record");
        assert!(read_run_record(&scratch.0).unwrap().is_none());
        std::fs::write(&scratch.0, b"{\"scheduled_for\":").unwrap();
        let error = read_run_record(&scratch.0).unwrap_err();
        assert!(error.contains("已停止自動執行"));
        assert!(error.contains("last-run.json"));
    }

    #[test]
    fn validates_day_selection_and_strict_24_hour_time() {
        let mut schedule = WeeklySchedule::default();
        for value in ["00:00", "12:59", "23:59"] {
            schedule.time = value.into();
            assert!(schedule.validate().is_ok());
        }
        for value in [
            "24:00",
            "12:60",
            "1:30",
            "23:30 /F",
            " 23:30",
            "23:30:00",
            "１２:３０",
        ] {
            schedule.time = value.into();
            assert!(schedule.validate().is_err(), "{value}");
        }
        schedule.time = "23:30".into();
        schedule.days = [false; 7];
        assert!(schedule.validate().is_err());
    }

    #[test]
    fn finds_next_selected_weekday_and_rolls_same_time_to_next_week() {
        let schedule = WeeklySchedule {
            days: [false, true, false, false, false, false, false],
            time: "23:30".into(),
            simulation: true,
        };
        assert_eq!(
            schedule
                .next_in_zone(&now())
                .unwrap()
                .format("%F %R")
                .to_string(),
            "2026-11-24 23:30"
        );
        let daily = WeeklySchedule::default();
        assert_eq!(
            daily
                .next_in_zone(&now())
                .unwrap()
                .format("%F %R")
                .to_string(),
            "2026-11-18 23:30"
        );
        let before = now() - chrono::Duration::seconds(1);
        assert_eq!(schedule.next_in_zone(&before).unwrap(), now());
    }

    #[test]
    fn weekday_selection_and_year_boundary_are_correct() {
        let date = now().with_month(12).unwrap().with_day(31).unwrap();
        let schedule = WeeklySchedule {
            days: [false, false, false, false, true, false, false],
            time: "00:00".into(),
            simulation: true,
        };
        assert_eq!(
            schedule
                .next_in_zone(&date)
                .unwrap()
                .format("%F %R")
                .to_string(),
            "2027-01-01 00:00"
        );
    }

    #[test]
    fn xml_uses_interactive_least_privilege_without_shell_or_password() {
        let schedule = WeeklySchedule {
            days: [true, false, true, false, false, false, false],
            ..Default::default()
        };
        let xml = task_xml(
            &schedule,
            Path::new(r#"C:\Some & Other\app.exe"#),
            "S-1-5-21-123",
            &now(),
        )
        .unwrap();
        assert!(xml.contains("<Monday/><Wednesday/>"));
        assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(xml.contains("C:\\Some &amp; Other\\app.exe"));
        assert!(xml.contains("--scheduled-dry-run"));
        assert!(!xml.contains("--scheduled-shutdown"));
        assert!(!xml.contains("cmd.exe"));
        assert!(!xml.contains("Password"));
        assert!(xml.contains("<StartWhenAvailable>false</StartWhenAvailable>"));
        assert!(xml.contains("<WakeToRun>false</WakeToRun>"));
    }

    #[test]
    fn real_xml_and_summaries_match_selected_modes() {
        let schedule = WeeklySchedule {
            simulation: false,
            ..Default::default()
        };
        assert_eq!(schedule.summary(), "每天 23:30");
        assert!(
            task_xml(&schedule, Path::new("app.exe"), "SID", &now())
                .unwrap()
                .contains("--scheduled-shutdown")
        );
        let weekdays = WeeklySchedule {
            days: [true, true, true, true, true, false, false],
            ..Default::default()
        };
        assert_eq!(weekdays.summary(), "每週一至五 23:30");
    }
}
