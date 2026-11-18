use chrono::{DateTime, Days, LocalResult, NaiveTime, TimeZone};
use std::time::{Duration, Instant};

pub mod schedule;
pub mod shutdown;

pub const MAX_MINUTES: u64 = 7 * 24 * 60;
pub const REMINDER_LEAD: Duration = Duration::from_secs(5 * 60);

pub fn parse_minutes(input: &str) -> Result<Duration, String> {
    let value = input.trim();
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("請輸入整數分鐘。".into());
    }
    let minutes: u64 = value.parse().map_err(|_| "分鐘數太大。")?;
    if !(1..=MAX_MINUTES).contains(&minutes) {
        return Err(format!("請輸入 1 至 {MAX_MINUTES} 分鐘（最多 7 天）。"));
    }
    Ok(Duration::from_secs(minutes * 60))
}

/// 指定時間一律取下一次出現的本機時刻；不猜測日光節約時間的歧義。
pub fn next_time<T: TimeZone>(now: &DateTime<T>, input: &str) -> Result<DateTime<T>, String> {
    let input = input.trim();
    let bytes = input.as_bytes();
    if bytes.len() != 5
        || bytes[2] != b':'
        || ![bytes[0], bytes[1], bytes[3], bytes[4]]
            .iter()
            .all(u8::is_ascii_digit)
    {
        return Err("請輸入 24 小時制 HH:MM，例如 23:30。".into());
    }
    let time = NaiveTime::parse_from_str(input, "%H:%M")
        .map_err(|_| "時間範圍錯誤，請使用 00:00 至 23:59。")?;
    let zone = now.timezone();
    let resolve = |date: chrono::NaiveDate| match zone.from_local_datetime(&date.and_time(time)) {
        LocalResult::Single(value) => Ok(value),
        _ => Err("這個時刻受到時區切換影響，請改用倒數分鐘。".to_owned()),
    };
    let today = now.date_naive();
    let target = resolve(today)?;
    if target > *now {
        Ok(target)
    } else {
        resolve(
            today
                .checked_add_days(Days::new(1))
                .ok_or("日期超出範圍。")?,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Simulation,
    Shutdown,
}

struct Countdown {
    started: Instant,
    duration: Duration,
    mode: Mode,
}

#[derive(Default)]
pub struct Timer {
    countdown: Option<Countdown>,
}

impl Timer {
    pub fn start(&mut self, now: Instant, duration: Duration, mode: Mode) -> Result<(), String> {
        if self.is_running() {
            return Err("已有倒數正在執行，請先取消。".into());
        }
        if duration.is_zero() || duration > Duration::from_secs(MAX_MINUTES * 60) {
            return Err("倒數時間必須大於零且不超過 7 天。".into());
        }
        self.countdown = Some(Countdown {
            started: now,
            duration,
            mode,
        });
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.countdown.is_some()
    }

    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        self.countdown.as_ref().map(|c| {
            c.duration
                .saturating_sub(now.saturating_duration_since(c.started))
        })
    }

    /// 延後目前這次倒數；若已到期但尚未執行，從現在開始給完整延長時間。
    pub fn extend(&mut self, now: Instant, additional: Duration) -> Result<Duration, String> {
        if additional.is_zero() {
            return Err("延長時間必須大於零。".into());
        }
        let remaining = self.remaining(now).ok_or("沒有正在執行的倒數。")?;
        let extended = remaining
            .checked_add(additional)
            .filter(|duration| *duration <= Duration::from_secs(MAX_MINUTES * 60))
            .ok_or("延長後的剩餘時間不得超過 7 天。")?;
        now.checked_add(extended).ok_or("延長時間超出系統範圍。")?;
        // 完成所有檢查後才修改，不讓失敗的操作改變原有到期時間或模式。
        let countdown = self.countdown.as_mut().ok_or("沒有正在執行的倒數。")?;
        countdown.started = now;
        countdown.duration = extended;
        Ok(extended)
    }

    pub fn should_remind(&self, now: Instant) -> bool {
        self.remaining(now)
            .is_some_and(|remaining| remaining <= REMINDER_LEAD)
    }

    pub fn cancel(&mut self) -> bool {
        self.countdown.take().is_some()
    }

    /// 先移除倒數再執行副作用，避免失敗後每幀重複關機。
    pub fn poll(
        &mut self,
        now: Instant,
        execute: impl FnOnce() -> Result<(), String>,
    ) -> Option<Result<Mode, String>> {
        if self.remaining(now)? > Duration::ZERO {
            return None;
        }
        let mode = self.countdown.take()?.mode;
        Some(match mode {
            Mode::Simulation => Ok(mode),
            Mode::Shutdown => execute().map(|()| mode),
        })
    }
}

pub fn format_remaining(duration: Duration) -> String {
    let seconds = duration.as_secs() + u64::from(duration.subsec_nanos() > 0);
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    #[test]
    fn validates_minutes_and_overflow() {
        for bad in [
            "",
            "0",
            "-1",
            "+1",
            "1.5",
            "10081",
            "18446744073709551616",
            "一",
        ] {
            assert!(parse_minutes(bad).is_err(), "{bad}");
        }
        assert_eq!(parse_minutes(" 30 ").unwrap(), Duration::from_secs(1800));
        assert_eq!(parse_minutes("10080").unwrap(), Duration::from_secs(604800));
    }

    #[test]
    fn clock_time_handles_next_day_and_year() {
        let tz = FixedOffset::east_opt(8 * 3600).unwrap();
        let now = tz.with_ymd_and_hms(2026, 12, 31, 23, 30, 0).unwrap();
        assert_eq!(
            next_time(&now, "23:31")
                .unwrap()
                .signed_duration_since(now)
                .num_seconds(),
            60
        );
        assert_eq!(
            next_time(&now, "23:30")
                .unwrap()
                .signed_duration_since(now)
                .num_hours(),
            24
        );
        assert_eq!(
            next_time(&now, "00:00")
                .unwrap()
                .format("%F %R")
                .to_string(),
            "2027-01-01 00:00"
        );
        for bad in [
            "24:00",
            "12:60",
            "1:30",
            "12:3",
            "12:300",
            "１２:３０",
            "no",
        ] {
            assert!(next_time(&now, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn timer_executes_once_only_at_deadline() {
        let mut timer = Timer::default();
        let now = Instant::now();
        timer
            .start(now, Duration::from_secs(2), Mode::Shutdown)
            .unwrap();
        assert!(
            timer
                .poll(now + Duration::from_secs(1), || panic!("early"))
                .is_none()
        );
        let mut calls = 0;
        assert_eq!(
            timer.poll(now + Duration::from_secs(2), || {
                calls += 1;
                Ok(())
            }),
            Some(Ok(Mode::Shutdown))
        );
        assert_eq!(calls, 1);
        assert!(
            timer
                .poll(now + Duration::from_secs(3), || panic!("repeated"))
                .is_none()
        );
    }

    #[test]
    fn cancellation_at_deadline_prevents_execution() {
        let mut timer = Timer::default();
        let now = Instant::now();
        timer
            .start(now, Duration::from_secs(1), Mode::Shutdown)
            .unwrap();
        assert!(timer.cancel());
        assert!(!timer.cancel());
        assert!(
            timer
                .poll(now + Duration::from_secs(1), || panic!("cancelled"))
                .is_none()
        );
    }

    #[test]
    fn simulation_never_calls_executor() {
        let mut timer = Timer::default();
        let now = Instant::now();
        timer
            .start(now, Duration::from_secs(1), Mode::Simulation)
            .unwrap();
        assert_eq!(
            timer.poll(now + Duration::from_secs(1), || panic!("real shutdown")),
            Some(Ok(Mode::Simulation))
        );
    }

    #[test]
    fn executor_failure_is_reported_without_retry() {
        let mut timer = Timer::default();
        let now = Instant::now();
        timer
            .start(now, Duration::from_secs(1), Mode::Shutdown)
            .unwrap();
        assert_eq!(
            timer.poll(now + Duration::from_secs(1), || Err("denied".into())),
            Some(Err("denied".into()))
        );
        assert!(!timer.is_running());
    }

    #[test]
    fn timer_rejects_invalid_duration_and_replacement() {
        let mut timer = Timer::default();
        let now = Instant::now();
        assert!(timer.start(now, Duration::ZERO, Mode::Shutdown).is_err());
        assert!(
            timer
                .start(now, Duration::from_secs(604801), Mode::Shutdown)
                .is_err()
        );
        timer
            .start(now, Duration::from_secs(10), Mode::Simulation)
            .unwrap();
        assert!(
            timer
                .start(now, Duration::from_secs(1), Mode::Shutdown)
                .is_err()
        );
        assert_eq!(timer.remaining(now).unwrap(), Duration::from_secs(10));
        assert_eq!(format_remaining(Duration::from_millis(1)), "00:00:01");
        assert_eq!(format_remaining(Duration::from_secs(3661)), "01:01:01");
    }

    #[test]
    fn extending_preserves_mode_and_executes_once_at_the_new_deadline() {
        let now = Instant::now();
        let mut timer = Timer::default();
        timer
            .start(now, Duration::from_secs(300), Mode::Shutdown)
            .unwrap();
        assert_eq!(
            timer.extend(now + Duration::from_secs(30), Duration::from_secs(600)),
            Ok(Duration::from_secs(870))
        );
        assert!(
            timer
                .poll(now + Duration::from_secs(300), || panic!("old deadline"))
                .is_none()
        );
        let mut calls = 0;
        assert_eq!(
            timer.poll(now + Duration::from_secs(900), || {
                calls += 1;
                Ok(())
            }),
            Some(Ok(Mode::Shutdown))
        );
        assert_eq!(calls, 1);
        assert!(
            timer
                .poll(now + Duration::from_secs(901), || panic!("duplicate"))
                .is_none()
        );
    }

    #[test]
    fn repeated_extensions_add_to_the_existing_deadline() {
        let now = Instant::now();
        let mut timer = Timer::default();
        timer
            .start(now, Duration::from_secs(300), Mode::Shutdown)
            .unwrap();
        timer.extend(now, Duration::from_secs(600)).unwrap();
        assert_eq!(
            timer.extend(now + Duration::from_secs(100), Duration::from_secs(1800)),
            Ok(Duration::from_secs(2600))
        );
        assert!(
            timer
                .poll(now + Duration::from_secs(900), || panic!("first extension"))
                .is_none()
        );
        assert_eq!(
            timer.poll(now + Duration::from_secs(2700), || Ok(())),
            Some(Ok(Mode::Shutdown))
        );
    }

    #[test]
    fn extension_at_or_after_expiry_takes_priority_over_poll() {
        for elapsed in [300, 301, 3600] {
            let now = Instant::now();
            let mut timer = Timer::default();
            timer
                .start(now, Duration::from_secs(300), Mode::Shutdown)
                .unwrap();
            let clicked = now + Duration::from_secs(elapsed);
            assert_eq!(
                timer.extend(clicked, Duration::from_secs(600)),
                Ok(Duration::from_secs(600))
            );
            assert!(timer.poll(clicked, || panic!("extended first")).is_none());
            assert_eq!(
                timer.poll(clicked + Duration::from_secs(600), || Ok(())),
                Some(Ok(Mode::Shutdown))
            );
        }
    }

    #[test]
    fn extending_a_simulation_never_invokes_the_shutdown_executor() {
        let now = Instant::now();
        let mut timer = Timer::default();
        timer
            .start(now, Duration::from_secs(60), Mode::Simulation)
            .unwrap();
        timer.extend(now, Duration::from_secs(600)).unwrap();
        assert_eq!(
            timer.poll(now + Duration::from_secs(660), || panic!("real shutdown")),
            Some(Ok(Mode::Simulation))
        );
    }

    #[test]
    fn invalid_extensions_leave_the_existing_timer_unchanged() {
        let now = Instant::now();
        let mut timer = Timer::default();
        assert!(timer.extend(now, Duration::from_secs(600)).is_err());
        timer
            .start(now, Duration::from_secs(60), Mode::Shutdown)
            .unwrap();
        for additional in [
            Duration::ZERO,
            Duration::MAX,
            Duration::from_secs(MAX_MINUTES * 60),
        ] {
            assert!(timer.extend(now, additional).is_err());
            assert_eq!(timer.remaining(now), Some(Duration::from_secs(60)));
        }
        assert!(timer.cancel());
        assert!(timer.extend(now, Duration::from_secs(600)).is_err());
        assert!(!timer.should_remind(now));
        assert!(
            timer
                .poll(now + Duration::from_secs(600), || panic!("cancelled"))
                .is_none()
        );
    }

    #[test]
    fn reminder_only_appears_during_the_last_five_minutes() {
        let now = Instant::now();
        let mut timer = Timer::default();
        assert!(!timer.should_remind(now));
        timer
            .start(now, Duration::from_secs(301), Mode::Simulation)
            .unwrap();
        assert!(!timer.should_remind(now));
        assert!(timer.should_remind(now + Duration::from_secs(1)));
        timer
            .extend(now + Duration::from_secs(1), Duration::from_secs(600))
            .unwrap();
        assert!(!timer.should_remind(now + Duration::from_secs(1)));
        assert!(timer.should_remind(now + Duration::from_secs(601)));
    }
}
