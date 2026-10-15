use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Idle,
    Running,
    Done,
    Error(String),
    Interrupted,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncProgress {
    pub state: State,
    pub files_total: i64,
    pub files_done: i64,
    pub current_file: String,
    pub bytes_total: i64,
    pub bytes_done: i64,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
}

impl Default for SyncProgress {
    fn default() -> Self {
        Self {
            state: State::Idle,
            files_total: 0,
            files_done: 0,
            current_file: String::new(),
            bytes_total: 0,
            bytes_done: 0,
            start_time: None,
            end_time: None,
        }
    }
}

impl SyncProgress {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fraction of files finished, or `0` when nothing is queued.
    pub fn percent_complete(&self) -> f64 {
        if self.files_total > 0 {
            self.files_done as f64 / self.files_total as f64
        } else {
            0.0
        }
    }

    /// Seconds since `start_time`. Uses `end_time` once the run has stopped, otherwise now.
    pub fn duration(&self) -> Option<f64> {
        let start = self.start_time?;
        let end = self.end_time.unwrap_or_else(Utc::now);
        Some((end - start).num_milliseconds() as f64 / 1000.0)
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            State::Done | State::Error(_) | State::Interrupted
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRecord {
    pub id: Uuid,
    pub date: DateTime<Utc>,
    pub file_count: i64,
    pub total_bytes: i64,
    pub duration_seconds: f64,
    pub succeeded: bool,
    /// Display name of the profile that produced this run. Older history has none.
    #[serde(default)]
    pub profile_name: String,
    /// Id of the profile that produced this run. Older history has none.
    #[serde(default)]
    pub profile_id: String,
    /// Why a failed run stopped. Absent on older history and on successes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SyncRecord {
    pub fn new(
        date: DateTime<Utc>,
        file_count: i64,
        total_bytes: i64,
        duration_seconds: f64,
        succeeded: bool,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            date,
            file_count,
            total_bytes,
            duration_seconds,
            succeeded,
            profile_name: String::new(),
            profile_id: String::new(),
            error: None,
        }
    }

    /// Short local date and time, e.g. `9/9/01, 1:46 AM`.
    pub fn formatted_date(&self) -> String {
        self.date
            .with_timezone(&Local)
            .format("%-m/%-d/%y, %-I:%M %p")
            .to_string()
    }

    pub fn formatted_size(&self) -> String {
        format_byte_count(self.total_bytes)
    }

    pub fn formatted_duration(&self) -> String {
        let d = (self.duration_seconds as i64).max(0);
        if d < 60 {
            format!("{d}s")
        } else {
            format!("{}m {}s", d / 60, d % 60)
        }
    }
}

fn format_byte_count(bytes: i64) -> String {
    let negative = bytes < 0;
    let n = bytes.unsigned_abs() as f64;
    const UNITS: [&str; 6] = ["bytes", "KB", "MB", "GB", "TB", "PB"];
    let mut value = n;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    let body = if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    };
    if negative {
        format!("-{body}")
    } else {
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_record_encodes_and_decodes() {
        let record = SyncRecord::new(
            DateTime::from_timestamp(1_000_000, 0).unwrap(),
            42,
            1_024,
            3.5,
            true,
        );
        let data = serde_json::to_vec(&record).unwrap();
        let decoded: SyncRecord = serde_json::from_slice(&data).unwrap();
        assert_eq!(decoded.file_count, 42);
        assert_eq!(decoded.total_bytes, 1_024);
        assert_eq!(decoded.duration_seconds, 3.5);
        assert!(decoded.succeeded);
        assert_eq!(decoded.id, record.id);
        assert_eq!(decoded.date, record.date);
        assert_eq!(record.formatted_duration(), "3s");
        assert_eq!(decoded.error, None);
        assert_eq!(decoded.profile_id, "");

        let mut failed = record.clone();
        failed.succeeded = false;
        failed.error = Some("The destination filled up.".into());
        let again: SyncRecord = serde_json::from_slice(&serde_json::to_vec(&failed).unwrap()).unwrap();
        assert_eq!(again.error.as_deref(), Some("The destination filled up."));

        let legacy = serde_json::json!({
            "id": record.id,
            "date": record.date,
            "fileCount": 1,
            "totalBytes": 2,
            "durationSeconds": 3.0,
            "succeeded": false,
            "profileName": "Photos"
        });
        let old: SyncRecord = serde_json::from_value(legacy).unwrap();
        assert_eq!(old.error, None);
        assert_eq!(old.profile_name, "Photos");
        assert_eq!(old.profile_id, "");

        let mut tagged = record.clone();
        tagged.profile_id = "profile-1".into();
        let with_id: SyncRecord =
            serde_json::from_slice(&serde_json::to_vec(&tagged).unwrap()).unwrap();
        assert_eq!(with_id.profile_id, "profile-1");
    }

    #[test]
    fn formatted_duration_minutes_and_negative() {
        let long = SyncRecord::new(Utc::now(), 1, 0, 90.9, true);
        assert_eq!(long.formatted_duration(), "1m 30s");
        let neg = SyncRecord::new(Utc::now(), 1, 0, -5.0, false);
        assert_eq!(neg.formatted_duration(), "0s");
    }

    #[test]
    fn default_state_is_idle() {
        let p = SyncProgress::new();
        assert_eq!(p.state, State::Idle);
        assert!(!p.is_terminal());
        let mut done = SyncProgress::new();
        done.state = State::Done;
        assert!(done.is_terminal());
    }

    #[test]
    fn percent_complete_zero_when_no_files() {
        let p = SyncProgress::new();
        assert_eq!(p.percent_complete(), 0.0);
    }

    #[test]
    fn percent_complete_calculates_correctly() {
        let mut p = SyncProgress::new();
        p.files_total = 100;
        p.files_done = 25;
        assert!((p.percent_complete() - 0.25).abs() < 0.001);
    }
}
