//! Exact-SHA latest push Check owner for the native maintenance model publisher.

use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};

use hyper::{
    Method,
    header::{ACCEPT, AUTHORIZATION, HeaderName, HeaderValue, USER_AGENT},
};
use url::Url;
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::{
    NativeOutboundHttp, NativeOutboundHttpRequest, NativeWirePrimitives,
    maintenance_canonical_json::{field, text},
};

const REPOSITORY_NAME: &str = "anstosa/weather";
const REPOSITORY_ID: u64 = 1_342_404_160;
const CHECK_PATH: &str = ".github/workflows/check.yml";

/// Source's categorical latest exact-push CI state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceModelCheckState {
    Success,
    Failure,
    Pending,
    Missing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceModelCheckError {
    Identity,
    Transport,
    Evidence,
    Failed,
    Deadline,
}

use NativeMaintenanceModelCheckError as Error;
use NativeMaintenanceModelCheckState as State;

/// Select only the newest trusted push Check for the exact commit and repository.
pub fn select_native_maintenance_model_check(
    document: &JsonValue,
    commit: &str,
) -> Result<State, Error> {
    // require one complete bounded page including empty catalogs
    let count = integer(document, "total_count").ok_or(Error::Evidence)?;
    let Some(JsonValue::Array(runs)) = field(document, "workflow_runs") else {
        return Err(Error::Evidence);
    };
    if count > 100 || runs.len() != count as usize || !valid_commit(commit) {
        return Err(Error::Evidence);
    }
    let mut latest = None;
    // examine every exact scoped run before admitting any success
    for run in runs {
        if text(run, "event").as_deref() != Some("push")
            || text(run, "head_sha").as_deref() != Some(commit)
            || !repository(field(run, "head_repository"))
        {
            continue;
        }
        let id = integer(run, "id").filter(|v| *v > 0);
        let attempt = integer(run, "run_attempt").filter(|v| *v > 0);
        let created_at = text(run, "created_at")
            .and_then(|value| NativeWirePrimitives.parse_explicit_utc_milliseconds(&value));
        let path = text(run, "path");
        // reject malformed same-repository evidence before sorting
        if text(run, "head_branch").is_none_or(|v| v.is_empty())
            || !repository(field(run, "repository"))
            || path.is_none_or(|path| !valid_check_path(&path))
            || attempt.is_none()
        {
            return Ok(State::Failure);
        }
        let Some((created_at, id)) = created_at.zip(id) else {
            return Ok(State::Failure);
        };
        let key = (created_at, id);
        // preserve first evidence on exactly equal timestamps and identities
        if latest.as_ref().is_none_or(|(current, _)| key > *current) {
            latest = Some((key, run));
        }
    }
    let Some((_, run)) = latest else {
        return Ok(State::Missing);
    };
    // distinguish a pending current run from a completed success or terminal failure
    Ok(match text(run, "status").as_deref() {
        Some("completed") if text(run, "conclusion").as_deref() == Some("success") => {
            State::Success
        }
        Some("queued" | "in_progress" | "requested" | "waiting" | "pending") => State::Pending,
        _ => State::Failure,
    })
}

/// Poll the installed authenticated GitHub API until the newest exact Check succeeds.
pub fn await_native_maintenance_model_check(
    client: &NativeOutboundHttp,
    commit: &str,
) -> Result<(), Error> {
    if !valid_commit(commit) {
        return Err(Error::Identity);
    }
    let deadline = Instant::now() + Duration::from_secs(30 * 60);
    // retain the predecessor's bounded polling interval
    while Instant::now() < deadline {
        match query(client, commit)? {
            State::Success => return Ok(()),
            State::Failure => return Err(Error::Failed),
            State::Pending | State::Missing => thread::sleep(Duration::from_secs(15)),
        }
    }
    Err(Error::Deadline)
}

/// Read one exact authenticated API state without exposing credentials.
fn query(client: &NativeOutboundHttp, commit: &str) -> Result<State, Error> {
    let token = Command::new("/home/linuxbrew/.linuxbrew/bin/gh")
        .args(["auth", "token", "--hostname", "github.com"])
        .env_clear()
        .env("GH_PROMPT_DISABLED", "1")
        .env("HOME", "/home/ubuntu")
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .env("PATH", "/usr/bin:/bin")
        .output()
        .map_err(|_| Error::Identity)?;
    if !token.status.success() || token.stdout.len() > 4096 || token.stderr.len() > 4096 {
        return Err(Error::Identity);
    }
    let token = String::from_utf8(token.stdout).map_err(|_| Error::Identity)?;
    let token = token.strip_suffix('\n').unwrap_or(&token);
    // admit only the source's single bounded token
    if !(20..=255).contains(&token.len()) || token.chars().any(char::is_whitespace) {
        return Err(Error::Identity);
    }
    let mut authorization =
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| Error::Identity)?;
    authorization.set_sensitive(true);
    let endpoint = Url::parse(&format!("https://api.github.com/repos/{REPOSITORY_NAME}/actions/workflows/check.yml/runs?head_sha={commit}&event=push&per_page=100"))
        .map_err(|_| Error::Transport)?;
    let request = NativeOutboundHttpRequest::new(Method::GET, endpoint)
        .map_err(|_| Error::Transport)?
        .header(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        )
        .header(AUTHORIZATION, authorization)
        .header(
            HeaderName::from_static("x-github-api-version"),
            HeaderValue::from_static("2026-03-10"),
        )
        .header(USER_AGENT, HeaderValue::from_static("weather-native/1"))
        .manual_redirect();
    let response = client
        .execute(request, Duration::from_secs(10), 4 * 1024 * 1024)
        .map_err(|_| Error::Transport)?;
    if response.status != 200 {
        return Err(Error::Transport);
    }
    let document = NativeWirePrimitives
        .decode_json(&response.body)
        .ok_or(Error::Evidence)?;
    select_native_maintenance_model_check(&document, commit)
}

/// Match the exact source repository id and spelling.
fn repository(value: Option<&JsonValue>) -> bool {
    value.is_some_and(|value| {
        integer(value, "id") == Some(REPOSITORY_ID)
            && text(value, "full_name").as_deref() == Some(REPOSITORY_NAME)
    })
}

/// Admit an exact JavaScript-safe integer.
fn integer(value: &JsonValue, key: &str) -> Option<u64> {
    let JsonValue::Number(number) = field(value, key)? else {
        return None;
    };
    (number.is_finite()
        && *number >= 0.0
        && *number <= 9_007_199_254_740_991.0
        && number.fract() == 0.0)
        .then_some(*number as u64)
}

/// Accept the workflow file itself or its ordinary source suffix.
fn valid_check_path(path: &str) -> bool {
    path == CHECK_PATH
        || path
            .strip_prefix(CHECK_PATH)
            .and_then(|suffix| suffix.strip_prefix('@'))
            .is_some_and(|suffix| !suffix.is_empty() && !suffix.chars().any(char::is_whitespace))
}

/// Admit only an exact complete lowercase Git object id.
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_push_run_matches_source_outcomes() {
        let commit = "a".repeat(40);
        let run = |id, date, status, conclusion| {
            format!(
                r#"{{"event":"push","head_sha":"{commit}","head_branch":"automation/adjustment-rain","head_repository":{{"id":1342404160,"full_name":"anstosa/weather"}},"repository":{{"id":1342404160,"full_name":"anstosa/weather"}},"path":".github/workflows/check.yml@refs/heads/main","id":{id},"run_attempt":1,"created_at":"{date}","status":"{status}","conclusion":"{conclusion}"}}"#
            )
        };
        let latest = |runs: &[String]| {
            let bytes = format!(
                r#"{{"total_count":{},"workflow_runs":[{}]}}"#,
                runs.len(),
                runs.join(",")
            );
            select_native_maintenance_model_check(
                &NativeWirePrimitives.decode_json(bytes.as_bytes()).unwrap(),
                &commit,
            )
        };
        let success = run(1, "2026-11-01T12:00:00Z", "completed", "success");
        let pending = run(2, "2026-11-01T12:01:00Z", "queued", "");
        let failure = run(3, "2026-11-01T12:02:00Z", "completed", "failure");
        assert_eq!(latest(&[]), Ok(State::Missing));
        assert_eq!(latest(std::slice::from_ref(&success)), Ok(State::Success));
        assert_eq!(
            latest(&[success.clone(), pending.clone()]),
            Ok(State::Pending)
        );
        assert_eq!(latest(&[failure, success, pending]), Ok(State::Failure));
    }
}
