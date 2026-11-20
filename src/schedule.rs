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
#[cfg(any(windows, test))]
// 保留 v2 識別字，避免開過舊版視窗時把新版背景程式換回去。
const REMINDER_MARKER: &str = "rust-auto-shutdown-reminder-v2; windows-notifications-v1";

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
    update_executable_if_needed(&source, &executable)?;
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

#[cfg(windows)]
fn update_executable_if_needed(source: &Path, target: &Path) -> Result<(), String> {
    let source_path = source
        .canonicalize()
        .map_err(|error| format!("無法讀取目前執行檔路徑：{error}"))?;
    if Some(source_path) == target.canonicalize().ok() || same_file_contents(source, target)? {
        return Ok(());
    }
    std::fs::copy(source, target)
        .map(|_bytes| ())
        .map_err(|error| {
            format!("無法更新排程執行檔：{error}。請先取消或關閉目前通知，再重新保存排程。")
        })
}

#[cfg(any(windows, test))]
fn same_file_contents(source: &Path, target: &Path) -> Result<bool, String> {
    let source_metadata =
        std::fs::metadata(source).map_err(|error| format!("無法讀取目前執行檔資訊：{error}"))?;
    let target_metadata = match std::fs::metadata(target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("無法讀取排程執行檔資訊：{error}")),
    };
    if source_metadata.len() != target_metadata.len() {
        return Ok(false);
    }
    let source_bytes =
        std::fs::read(source).map_err(|error| format!("無法比較目前執行檔：{error}"))?;
    let target_bytes =
        std::fs::read(target).map_err(|error| format!("無法比較排程執行檔：{error}"))?;
    Ok(source_bytes == target_bytes)
}

#[cfg(not(windows))]
pub fn save(_schedule: &WeeklySchedule) -> Result<(), String> {
    Err("每週排程僅支援 Windows。".into())
}

#[derive(Debug)]
pub enum SaveOutcome {
    Scheduled,
    ReminderStarted,
    ReminderFailed(String),
}

/// 明確儲存時，若下一次已進入提醒時段，由獨立通知程序接手當次。
pub fn save_from_ui(schedule: &WeeklySchedule) -> Result<SaveOutcome, String> {
    let occurrence = schedule.next_occurrence(&Local::now())?;
    save(schedule)?;
    let now = Local::now();
    if occurrence.signed_duration_since(now) > reminder_lead() {
        return Ok(SaveOutcome::Scheduled);
    }
    let launch = || -> Result<bool, String> {
        if !immediate_window(&occurrence, &now) {
            return Err("儲存時已超過這次的提醒時段。".into());
        }
        if last_run()?.is_some_and(|record| record.scheduled_for == occurrence_key(&occurrence)) {
            return Ok(false);
        }
        let mode = if schedule.simulation {
            "--scheduled-dry-run"
        } else {
            "--scheduled-shutdown"
        };
        std::process::Command::new(data_directory()?.join("app.exe"))
            .args([mode, "--remind-now", &occurrence.to_rfc3339()])
            .spawn()
            .map_err(|error| format!("無法開啟本次通知：{error}"))?;
        Ok(true)
    };
    Ok(match launch() {
        Ok(true) => SaveOutcome::ReminderStarted,
        Ok(false) => SaveOutcome::Scheduled,
        Err(error) => SaveOutcome::ReminderFailed(error),
    })
}

fn reminder_lead() -> chrono::Duration {
    chrono::Duration::from_std(crate::REMINDER_LEAD).expect("five minutes fit chrono::Duration")
}

fn immediate_window<T: TimeZone>(occurrence: &DateTime<T>, now: &DateTime<T>) -> bool {
    // 登錄任務可能剛好跨過整分鐘。只接受這個明確選定的時刻，並限制啟動延遲。
    let remaining = occurrence.clone().signed_duration_since(now.clone());
    (-chrono::Duration::seconds(30)..=reminder_lead()).contains(&remaining)
}

fn occurrence_key<T: TimeZone>(target: &DateTime<T>) -> String {
    target.naive_local().format("%Y-%m-%d %H:%M").to_string()
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

/// 更新舊版任務的提醒方式及背景執行檔。
#[cfg(windows)]
pub fn needs_reminder_update() -> Result<bool, String> {
    Ok(query_task_xml(&task_name()?)?.is_some_and(|xml| !xml.contains(REMINDER_MARKER)))
}

#[cfg(not(windows))]
pub fn needs_reminder_update() -> Result<bool, String> {
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

/// 已領取的單次提醒。延後只更新 `target`，不改 `scheduled_for` 與固定週期。
pub struct ScheduledReminder {
    pub target: DateTime<Local>,
    pub simulation: bool,
    pub scheduled_for: String,
    claim_lock: Option<FileLock>,
    record_path: PathBuf,
    finished: bool,
}

impl ScheduledReminder {
    pub fn finish(&mut self, result: Result<(), String>) -> Result<(), String> {
        let (success, message) = match result {
            Ok(()) if self.simulation => (true, "模擬完成，未執行關機。".into()),
            Ok(()) => (true, "Windows 已接受關機要求。".into()),
            Err(error) => (false, format!("執行失敗：{error}")),
        };
        self.complete(success, message)
    }

    pub fn cancel(&mut self) -> Result<(), String> {
        self.complete(true, "已取消這次關機，固定每週排程保持不變。".into())
    }

    fn complete(&mut self, success: bool, message: String) -> Result<(), String> {
        if self.finished {
            return Err("這次提醒已結束，未重複更新。".into());
        }
        let record = RunRecord {
            scheduled_for: self.scheduled_for.clone(),
            completed_at: Local::now().to_rfc3339(),
            simulation: self.simulation,
            success,
            message,
        };
        // 即使最後紀錄失敗，先前的領取紀錄仍阻止同一次週期再次執行。
        let result = write_run_record(&self.record_path, &record);
        self.finished = true;
        self.claim_lock.take();
        result
    }
}

/// 一般排程只接受原通知點後兩分鐘內啟動。即時提醒另核對指定週期。
pub fn prepare_reminder(
    simulation: bool,
    immediate: Option<&str>,
) -> Result<Option<ScheduledReminder>, String> {
    let now = Local::now();
    let schedule = load()?.ok_or("沒有已啟用的每週排程。")?;
    if simulation != schedule.simulation {
        return Err("工作模式與已保存設定不符，拒絕執行。".into());
    }
    let directory = data_directory()?;
    if let Some(value) = immediate {
        let target = DateTime::parse_from_rfc3339(value)
            .map_err(|_| "本次提醒時間格式錯誤。")?
            .with_timezone(&Local);
        if !matches_immediate_occurrence(&schedule, &target, &now)? {
            return Ok(None);
        }
        claim_occurrence(&schedule, target, &now, &directory)
    } else {
        claim_reminder(&schedule, &now, &directory)
    }
}

fn matches_immediate_occurrence<T: TimeZone>(
    schedule: &WeeklySchedule,
    target: &DateTime<T>,
    now: &DateTime<T>,
) -> Result<bool, String> {
    schedule.validate()?;
    Ok(immediate_window(target, now)
        && schedule.days[target.weekday().num_days_from_monday() as usize]
        && target.time() == parse_clock(&schedule.time)?)
}

fn reminder_target<T: TimeZone>(
    schedule: &WeeklySchedule,
    now: &DateTime<T>,
) -> Result<Option<DateTime<T>>, String> {
    schedule.validate()?;
    let time = parse_clock(&schedule.time)?;
    // 提醒可能在關機日期的前一天，最多需檢查今天與明天的關機時刻。
    for offset in 0..=1 {
        let date = now
            .date_naive()
            .checked_add_days(Days::new(offset))
            .ok_or("日期超出範圍。")?;
        if !schedule.days[date.weekday().num_days_from_monday() as usize] {
            continue;
        }
        let target = match now.timezone().from_local_datetime(&date.and_time(time)) {
            LocalResult::Single(target) => target,
            _ => return Err("提醒時刻受到時區切換影響，未執行。".into()),
        };
        let notify_at = target.clone() - reminder_lead();
        let elapsed = now.clone().signed_duration_since(notify_at);
        if elapsed >= chrono::Duration::zero() && elapsed < chrono::Duration::minutes(2) {
            return Ok(Some(target));
        }
    }
    Ok(None)
}

fn claim_reminder(
    schedule: &WeeklySchedule,
    now: &DateTime<Local>,
    directory: &Path,
) -> Result<Option<ScheduledReminder>, String> {
    let Some(target) = reminder_target(schedule, now)? else {
        return Ok(None);
    };
    claim_occurrence(schedule, target, now, directory)
}

fn claim_occurrence(
    schedule: &WeeklySchedule,
    target: DateTime<Local>,
    now: &DateTime<Local>,
    directory: &Path,
) -> Result<Option<ScheduledReminder>, String> {
    let lock = FileLock::acquire(directory.join("execution.lock"))?;
    let scheduled_for = occurrence_key(&target);
    let record_path = directory.join("last-run.json");
    if read_run_record(&record_path)?
        .is_some_and(|previous| previous.scheduled_for == scheduled_for)
    {
        return Ok(None);
    }
    write_run_record(
        &record_path,
        &RunRecord {
            scheduled_for: scheduled_for.clone(),
            completed_at: now.to_rfc3339(),
            simulation: schedule.simulation,
            success: false,
            message: "已開啟關機前提醒，等待倒數或使用者操作。".into(),
        },
    )?;
    Ok(Some(ScheduledReminder {
        target,
        simulation: schedule.simulation,
        scheduled_for,
        claim_lock: Some(lock),
        record_path,
        finished: false,
    }))
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
fn notification_pattern(schedule: &WeeklySchedule) -> Result<(NaiveTime, [bool; 7]), String> {
    schedule.validate()?;
    let time = parse_clock(&schedule.time)?;
    let (notify_time, overflow) = time.overflowing_sub_signed(chrono::Duration::minutes(5));
    let days = if overflow != 0 {
        let mut shifted = [false; 7];
        for (index, selected) in schedule.days.iter().enumerate() {
            shifted[(index + 6) % 7] = *selected;
        }
        shifted
    } else {
        schedule.days
    };
    Ok((notify_time, days))
}

#[cfg(any(windows, test))]
fn task_xml<T: TimeZone>(
    schedule: &WeeklySchedule,
    executable: &Path,
    sid: &str,
    now: &DateTime<T>,
) -> Result<String, String> {
    let (notify_time, notification_days) = notification_pattern(schedule)?;
    let executable = executable
        .to_str()
        .ok_or("排程執行檔路徑不是有效 Unicode。")?;
    let selected_days = notification_days
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
  <RegistrationInfo><Description>R Auto Shutdown weekly schedule for the current user. {REMINDER_MARKER}</Description></RegistrationInfo>
  <Triggers><CalendarTrigger><StartBoundary>{date}T{time}:00</StartBoundary><Enabled>true</Enabled><ScheduleByWeek><WeeksInterval>1</WeeksInterval><DaysOfWeek>{selected_days}</DaysOfWeek></ScheduleByWeek></CalendarTrigger></Triggers>
  <Principals><Principal id="CurrentUser"><UserId>{sid}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings><MultipleInstancesPolicy>Parallel</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><AllowHardTerminate>false</AllowHardTerminate><StartWhenAvailable>false</StartWhenAvailable><RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable><AllowStartOnDemand>false</AllowStartOnDemand><Enabled>true</Enabled><Hidden>false</Hidden><RunOnlyIfIdle>false</RunOnlyIfIdle><WakeToRun>false</WakeToRun><ExecutionTimeLimit>PT0S</ExecutionTimeLimit><Priority>7</Priority></Settings>
  <Actions Context="CurrentUser"><Exec><Command>{executable}</Command><Arguments>{argument}</Arguments></Exec></Actions>
</Task>"#,
        date = now.date_naive().format("%Y-%m-%d"),
        time = notify_time.format("%H:%M"),
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
            let _ = std::fs::remove_file(self.0.with_extension("copy"));
            let _ = std::fs::remove_file(self.0.parent().unwrap().join("schedule.json"));
            let _ = std::fs::remove_dir(self.0.parent().unwrap());
        }
    }

    #[test]
    fn executable_comparison_handles_missing_different_and_identical_files() {
        let scratch = ScratchRecord::new("binary-comparison");
        let target = scratch.0.with_extension("copy");
        std::fs::write(&scratch.0, b"binary-v1").unwrap();
        assert!(!same_file_contents(&scratch.0, &target).unwrap());
        std::fs::write(&target, b"a-different-length").unwrap();
        assert!(!same_file_contents(&scratch.0, &target).unwrap());
        std::fs::write(&target, b"binary-v2").unwrap();
        assert!(!same_file_contents(&scratch.0, &target).unwrap());
        std::fs::write(&target, b"binary-v1").unwrap();
        assert!(same_file_contents(&scratch.0, &target).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn saving_identical_binary_does_not_overwrite_locked_notification_executable() {
        use std::os::windows::fs::OpenOptionsExt;

        let scratch = ScratchRecord::new("locked-notification");
        let target = scratch.0.with_extension("copy");
        std::fs::write(&scratch.0, b"binary-v1").unwrap();
        std::fs::write(&target, b"binary-v1").unwrap();
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&target)
            .unwrap();
        update_executable_if_needed(&scratch.0, &target).unwrap();
        std::fs::write(&scratch.0, b"binary-v2").unwrap();
        let error = update_executable_if_needed(&scratch.0, &target).unwrap_err();
        assert!(error.contains("請先取消或關閉目前通知"));
        assert_eq!(std::fs::read(&target).unwrap(), b"binary-v1");
        drop(locked);
        update_executable_if_needed(&scratch.0, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"binary-v2");
    }

    #[test]
    fn notification_pattern_moves_early_monday_shutdown_to_sunday() {
        let mut schedule = WeeklySchedule {
            days: [true, false, true, false, false, false, false],
            time: "00:02".into(),
            simulation: true,
        };
        let (time, days) = notification_pattern(&schedule).unwrap();
        assert_eq!(time.format("%H:%M").to_string(), "23:57");
        assert_eq!(days, [false, true, false, false, false, false, true]);
        schedule.time = "00:05".into();
        let (time, days) = notification_pattern(&schedule).unwrap();
        assert_eq!(time.format("%H:%M").to_string(), "00:00");
        assert_eq!(days, schedule.days);
    }

    #[test]
    fn reminder_window_resolves_week_month_and_year_crossings() {
        let zone = FixedOffset::east_opt(8 * 3600).unwrap();
        for (year, month, day, expected, weekday) in [
            (2026, 11, 22, "2026-11-23 00:02", 0),
            (2027, 2, 28, "2027-03-01 00:02", 0),
            (2026, 12, 31, "2027-01-01 00:02", 4),
        ] {
            let mut schedule = WeeklySchedule {
                days: [false; 7],
                time: "00:02".into(),
                simulation: true,
            };
            schedule.days[weekday] = true;
            let notification = zone.with_ymd_and_hms(year, month, day, 23, 57, 0).unwrap();
            let target = reminder_target(&schedule, &notification).unwrap().unwrap();
            assert_eq!(target.format("%F %R").to_string(), expected);
            assert!(
                reminder_target(
                    &schedule,
                    &(notification - chrono::Duration::milliseconds(1))
                )
                .unwrap()
                .is_none()
            );
            assert!(
                reminder_target(
                    &schedule,
                    &(notification + chrono::Duration::milliseconds(119_999))
                )
                .unwrap()
                .is_some()
            );
            assert!(
                reminder_target(&schedule, &(notification + chrono::Duration::minutes(2)))
                    .unwrap()
                    .is_none()
            );
            assert!(reminder_target(&schedule, &target).unwrap().is_none());
        }
    }

    #[test]
    fn cancellation_claims_only_this_occurrence_and_keeps_weekly_config() {
        let scratch = ScratchRecord::new("cancel-reminder");
        let directory = scratch.0.parent().unwrap();
        let schedule = WeeklySchedule {
            time: "12:00".into(),
            ..Default::default()
        };
        let config = serde_json::to_vec_pretty(&schedule).unwrap();
        let config_path = directory.join("schedule.json");
        std::fs::write(&config_path, &config).unwrap();
        let now = Local.with_ymd_and_hms(2026, 11, 18, 11, 55, 0).unwrap();
        let mut reminder = claim_reminder(&schedule, &now, directory).unwrap().unwrap();
        assert!(claim_reminder(&schedule, &now, directory).is_err());
        reminder.target += chrono::Duration::minutes(30);
        assert_eq!(reminder.scheduled_for, "2026-11-18 12:00");
        reminder.cancel().unwrap();
        assert!(
            claim_reminder(&schedule, &now, directory)
                .unwrap()
                .is_none()
        );
        assert_eq!(std::fs::read(config_path).unwrap(), config);
        let record = read_run_record(&scratch.0).unwrap().unwrap();
        assert_eq!(record.scheduled_for, "2026-11-18 12:00");
        assert!(record.message.contains("已取消這次關機"));
        assert!(reminder.finish(Ok(())).is_err());
    }

    #[test]
    fn reminder_finish_records_simulation_and_does_not_reclaim_after_drop() {
        let scratch = ScratchRecord::new("finish-reminder");
        let directory = scratch.0.parent().unwrap();
        let schedule = WeeklySchedule {
            time: "12:00".into(),
            ..Default::default()
        };
        let now = Local.with_ymd_and_hms(2026, 11, 18, 11, 55, 0).unwrap();
        let mut reminder = claim_reminder(&schedule, &now, directory).unwrap().unwrap();
        reminder.target += chrono::Duration::minutes(10);
        reminder.finish(Ok(())).unwrap();
        // 錯誤或結果視窗即使仍持有 session，下一次提醒也能取得執行鎖。
        let released = FileLock::acquire(directory.join("execution.lock")).unwrap();
        drop(released);
        let record = read_run_record(&scratch.0).unwrap().unwrap();
        assert!(record.success && record.simulation);
        assert_eq!(record.message, "模擬完成，未執行關機。");
        assert_eq!(record.scheduled_for, "2026-11-18 12:00");
        drop(reminder);
        assert!(
            claim_reminder(&schedule, &now, directory)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn saving_seventeen_seconds_before_midnight_due_time_keeps_todays_occurrence() {
        let schedule = WeeklySchedule {
            time: "00:04".into(),
            ..WeeklySchedule::default()
        };
        let now = Local.with_ymd_and_hms(2026, 11, 19, 0, 3, 43).unwrap();
        let target = schedule.next_occurrence(&now).unwrap();
        assert_eq!(target.format("%F %T").to_string(), "2026-11-19 00:04:00");
        assert!(reminder_target(&schedule, &now).unwrap().is_none());
        assert!(matches_immediate_occurrence(&schedule, &target, &now).unwrap());
        assert!(
            !matches_immediate_occurrence(
                &schedule,
                &target,
                &(target + chrono::Duration::seconds(31))
            )
            .unwrap()
        );
        let scratch = ScratchRecord::new("last-minute-save");
        let directory = scratch.0.parent().unwrap();
        let mut reminder = claim_occurrence(&schedule, target, &now, directory)
            .unwrap()
            .unwrap();
        assert_eq!(reminder.scheduled_for, "2026-11-19 00:04");
        reminder.cancel().unwrap();
        assert!(
            claim_occurrence(&schedule, target, &now, directory)
                .unwrap()
                .is_none()
        );
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
        assert!(xml.contains("T23:25:00</StartBoundary>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains(REMINDER_MARKER));
        assert!(xml.contains("<MultipleInstancesPolicy>Parallel</MultipleInstancesPolicy>"));
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
