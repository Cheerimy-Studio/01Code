//! 小工具:时间显示 / 相对时间

use chrono::Local;

/// 当前本地时间 HH:MM
pub fn now_hhmm() -> String {
    Local::now().format("%H:%M").to_string()
}

/// Unix 秒 → 「刚刚 / N 分钟 / N 小时 / N 天」
pub fn rel_time(unix_secs: u64) -> String {
    let now = Local::now().timestamp().max(0) as u64;
    let d = now.saturating_sub(unix_secs);
    if d < 60 {
        "刚刚".to_string()
    } else if d < 3600 {
        format!("{}分钟", d / 60)
    } else if d < 86400 {
        format!("{}小时", d / 3600)
    } else {
        format!("{}天", d / 86400)
    }
}
