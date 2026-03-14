use chrono::{Local, TimeZone};
use serde::Deserialize;
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const SHOW_MODEL: bool = true;
const SHOW_DIRECTORY: bool = true;
const SHOW_BRANCH: bool = true;
const SHOW_CONTEXT: bool = true;
const SHOW_USAGE: bool = true;
const SHOW_PROGRESS_BAR: bool = true;
const SHOW_PACE_MARKER: bool = true;
const SHOW_RESET_TIME: bool = true;
const SHOW_WEEKLY: bool = true;
const SHOW_EXTRA_USAGE: bool = true;
const CURRENCY_SYMBOL: &str = "€";
const CACHE_MAX_AGE: u64 = 180;
const SEP: &str = " │ ";

const BAR_FULL: &str = "██████████";
const BAR_HALF: &str = "▓";
const BAR_EMPTY: &str = "░░░░░░░░░░";

#[derive(Deserialize, Default)]
struct StdinInput {
    #[serde(default)]
    workspace: Workspace,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    model: Model,
    #[serde(default)]
    context_window: ContextWindow,
}

#[derive(Deserialize, Default)]
struct Workspace {
    #[serde(default)]
    current_dir: String,
}

#[derive(Deserialize, Default)]
struct Model {
    #[serde(default)]
    display_name: String,
}

#[derive(Deserialize, Default)]
struct ContextWindow {
    #[serde(default)]
    context_window_size: u64,
    #[serde(default)]
    current_usage: Option<CurrentUsage>,
}

#[derive(Deserialize, Default)]
struct CurrentUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

#[derive(Default)]
struct UsageCache {
    timestamp: u64,
    utilization: Option<i32>,
    resets_at: String,
    weekly_utilization: Option<i32>,
    weekly_resets_at: String,
    extra_enabled: bool,
    extra_used: String,
}

#[derive(Deserialize)]
struct UsageResponse {
    #[serde(default)]
    five_hour: Option<UsagePeriod>,
    #[serde(default)]
    seven_day: Option<UsagePeriod>,
    #[serde(default)]
    extra_usage: Option<ExtraUsage>,
}

#[derive(Deserialize)]
struct UsagePeriod {
    #[serde(default)]
    utilization: f64,
    #[serde(default)]
    resets_at: String,
}

#[derive(Deserialize)]
struct ExtraUsage {
    #[serde(default)]
    is_enabled: bool,
    #[serde(default)]
    used_credits: f64,
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn cache_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".cache/claude-statusline")
}

fn parse_cache_file(contents: &str) -> UsageCache {
    let mut cache = UsageCache::default();
    for line in contents.lines() {
        if let Some((key, val)) = line.split_once('=') {
            match key {
                "TIMESTAMP" => cache.timestamp = val.parse().unwrap_or(0),
                "UTILIZATION" => cache.utilization = val.parse().ok(),
                "RESETS_AT" => cache.resets_at = val.to_string(),
                "WEEKLY_UTILIZATION" => cache.weekly_utilization = val.parse().ok(),
                "WEEKLY_RESETS_AT" => cache.weekly_resets_at = val.to_string(),
                "EXTRA_ENABLED" => cache.extra_enabled = val == "1",
                "EXTRA_USED" => cache.extra_used = val.to_string(),
                _ => {}
            }
        }
    }
    cache
}

fn read_cache(now: u64) -> UsageCache {
    let dir = cache_dir();
    let cache_file = dir.join("usage.cache");

    let mut cache = if let Ok(contents) = fs::read_to_string(&cache_file) {
        parse_cache_file(&contents)
    } else {
        UsageCache::default()
    };

    let fresh = cache.timestamp > 0 && (now - cache.timestamp) < CACHE_MAX_AGE;
    if fresh {
        return cache;
    }

    let lock_file = dir.join("fetch.lock");
    let locked = fs::read_to_string(&lock_file)
        .ok()
        .and_then(|c| c.trim().parse::<u64>().ok())
        .is_some_and(|exp| now < exp);

    if !locked {
        if fetch_usage(now).is_ok() {
            if let Ok(contents) = fs::read_to_string(&cache_file) {
                cache = parse_cache_file(&contents);
            }
        }
    }

    cache
}

fn fetch_usage(now: u64) -> Result<(), ()> {
    let dir = cache_dir();
    let _ = fs::create_dir_all(&dir);
    let lock_file = dir.join("fetch.lock");
    let _ = fs::write(&lock_file, format!("{}", now + 30));

    let cred_output = Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .output()
        .map_err(|_| ())?;

    if !cred_output.status.success() {
        return Err(());
    }

    let cred_json: serde_json::Value =
        serde_json::from_slice(&cred_output.stdout).map_err(|_| ())?;

    let token = cred_json
        .get("claudeAiOauth")
        .and_then(|o| o.get("accessToken"))
        .and_then(|t| t.as_str())
        .ok_or(())?;

    if token.is_empty() {
        return Err(());
    }

    let resp = ureq::get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .call();

    let mut resp = match resp {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(429)) => {
            let _ = fs::write(&lock_file, format!("{}", now + 300));
            return Err(());
        }
        Err(_) => return Err(()),
    };

    let body: UsageResponse = resp.body_mut().read_json().map_err(|_| ())?;

    let fh = body.five_hour.unwrap_or(UsagePeriod {
        utilization: 0.0,
        resets_at: String::new(),
    });
    let sd = body.seven_day.unwrap_or(UsagePeriod {
        utilization: 0.0,
        resets_at: String::new(),
    });
    let eu = body.extra_usage.unwrap_or(ExtraUsage {
        is_enabled: false,
        used_credits: 0.0,
    });

    let cache_content = format!(
        "TIMESTAMP={}\nUTILIZATION={}\nRESETS_AT={}\nWEEKLY_UTILIZATION={}\nWEEKLY_RESETS_AT={}\nEXTRA_ENABLED={}\nEXTRA_USED={:.2}",
        now,
        fh.utilization.round() as i32,
        fh.resets_at,
        sd.utilization.round() as i32,
        sd.resets_at,
        if eu.is_enabled { 1 } else { 0 },
        eu.used_credits / 100.0,
    );

    let _ = fs::write(dir.join("usage.cache"), cache_content);
    Ok(())
}

fn build_bar(util: i32) -> String {
    let util = util.clamp(0, 100);
    let half_steps = (util * 20 / 100) as usize;
    let full = half_steps / 2;
    let has_half = half_steps % 2;
    let empty = 10 - full - has_half;

    let full_chars: String = BAR_FULL.chars().take(full).collect();
    let empty_chars: String = BAR_EMPTY.chars().take(empty).collect();

    let mut bar = format!(" {full_chars}");
    if has_half == 1 {
        bar.push_str(BAR_HALF);
    }
    bar.push_str(&empty_chars);
    bar
}

fn insert_pace_marker(bar: &str, remaining: i64, window: i64) -> String {
    let elapsed = window - remaining;
    let marker_pos = ((elapsed * 10 + window / 2) / window).clamp(0, 9) as usize;
    let pos = marker_pos + 1;

    let chars: Vec<char> = bar.chars().collect();
    let mut result = String::with_capacity(chars.len() * 4);
    for (i, &c) in chars.iter().enumerate() {
        if i == pos {
            result.push('┃');
        } else {
            result.push(c);
        }
    }
    result
}

fn parse_reset_epoch(iso: &str) -> Option<i64> {
    if iso.len() < 19 {
        return None;
    }
    chrono::NaiveDateTime::parse_from_str(&iso[..19], "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|ndt| ndt.and_utc().timestamp())
}

fn round_epoch_to_minute(epoch: i64) -> i64 {
    let secs = epoch % 60;
    if secs >= 30 {
        epoch + (60 - secs)
    } else {
        epoch - secs
    }
}

fn format_time_hm(epoch: i64) -> Option<String> {
    Local
        .timestamp_opt(epoch, 0)
        .single()
        .map(|dt| dt.format("%H:%M").to_string())
}

fn format_time_day_hm(epoch: i64) -> Option<String> {
    Local
        .timestamp_opt(epoch, 0)
        .single()
        .map(|dt| dt.format("%a %H:%M").to_string())
}

fn get_git_branch() -> Option<String> {
    let try_read_head = |path: &str| -> Option<String> {
        let content = fs::read_to_string(path).ok()?;
        let line = content.lines().next()?;
        Some(
            line.strip_prefix("ref: refs/heads/")
                .unwrap_or(line)
                .to_string(),
        )
    };

    if let Some(branch) = try_read_head(".git/HEAD") {
        return Some(branch);
    }

    let output = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let git_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    try_read_head(&format!("{git_dir}/HEAD"))
}

fn strip_model_parens(name: &str) -> &str {
    match name.find('(') {
        Some(pos) => name[..pos].trim_end(),
        None => name,
    }
}

fn main() {
    let now = now_epoch();

    let mut stdin_buf = String::new();
    if io::stdin().read_to_string(&mut stdin_buf).is_err() {
        return;
    }

    let input: StdinInput = serde_json::from_str(&stdin_buf).unwrap_or_default();

    let current_dir_path = if !input.workspace.current_dir.is_empty() {
        &input.workspace.current_dir
    } else {
        &input.cwd
    };
    let current_dir = current_dir_path
        .rsplit('/')
        .next()
        .unwrap_or(current_dir_path);

    let cu = input.context_window.current_usage.unwrap_or_default();
    let cache = read_cache(now);

    let mut segments: Vec<String> = Vec::new();

    if SHOW_DIRECTORY && !current_dir.is_empty() {
        segments.push(current_dir.to_string());
    }

    if SHOW_BRANCH {
        if let Some(branch) = get_git_branch() {
            if !branch.is_empty() {
                segments.push(format!("⎇ {branch}"));
            }
        }
    }

    if SHOW_MODEL && !input.model.display_name.is_empty() {
        segments.push(strip_model_parens(&input.model.display_name).to_string());
    }

    if SHOW_CONTEXT && input.context_window.context_window_size > 0 {
        let total = cu.input_tokens + cu.cache_creation_input_tokens + cu.cache_read_input_tokens;
        let pct = total * 100 / input.context_window.context_window_size;
        segments.push(format!("{pct}%"));
    }

    if SHOW_USAGE {
        if let Some(util) = cache.utilization {
            let mut seg = format!("{util}%");
            let reset_epoch = parse_reset_epoch(&cache.resets_at);

            if SHOW_PROGRESS_BAR {
                let mut bar = build_bar(util);
                if SHOW_PACE_MARKER {
                    if let Some(re) = reset_epoch {
                        let remaining = re - now as i64;
                        if remaining > 0 && remaining < 18000 {
                            bar = insert_pace_marker(&bar, remaining, 18000);
                        }
                    }
                }
                seg.push_str(&bar);
            }

            if SHOW_RESET_TIME {
                if let Some(re) = reset_epoch {
                    let rounded = round_epoch_to_minute(re);
                    if let Some(display) = format_time_hm(rounded) {
                        seg.push(' ');
                        seg.push_str(&display);
                    }
                }
            }

            segments.push(seg);
        }
    }

    if SHOW_WEEKLY {
        if let Some(util) = cache.weekly_utilization {
            let mut seg = format!("{util}%");
            let reset_epoch = parse_reset_epoch(&cache.weekly_resets_at);

            if SHOW_PROGRESS_BAR {
                let mut bar = build_bar(util);
                if SHOW_PACE_MARKER {
                    if let Some(re) = reset_epoch {
                        let remaining = re - now as i64;
                        if remaining > 0 && remaining < 604800 {
                            bar = insert_pace_marker(&bar, remaining, 604800);
                        }
                    }
                }
                seg.push_str(&bar);
            }

            if SHOW_RESET_TIME {
                if let Some(re) = reset_epoch {
                    let rounded = round_epoch_to_minute(re);
                    if let Some(display) = format_time_day_hm(rounded) {
                        seg.push(' ');
                        seg.push_str(&display);
                    }
                }
            }

            segments.push(seg);
        }
    }

    if SHOW_EXTRA_USAGE && cache.extra_enabled && !cache.extra_used.is_empty() {
        segments.push(format!("{CURRENCY_SYMBOL}{}", cache.extra_used));
    }

    println!("{}", segments.join(SEP));
}
