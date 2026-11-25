//! Detached, exact model-release commit preparation and immutable Git references.

use std::{
    collections::BTreeSet,
    fs,
    io::ErrorKind,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{
    NativeMaintenanceDevelopmentPublication, NativeMaintenanceDevelopmentPublicationFile,
    maintenance_canonical_json::{text, valid_hash},
};

const SELECTOR: &str = "config/forecast-adjustments/ballydidean-rain-control-reference.json";
static NEXT: AtomicU64 = AtomicU64::new(0);

/// Exact prepared commit identity before any immutable ref is published.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceModelReleaseCommit {
    pub commit_sha: String,
    pub branch: String,
    pub action_tag: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceModelReleaseGitError {
    Input,
    Environment,
    Worktree,
    Package,
    Git,
    Collision,
    Namespace,
}

use NativeMaintenanceModelReleaseGitError as Error;

/// Own the disposable detached worktree until the exact preparation finishes.
struct Worktree {
    repository: PathBuf,
    root: PathBuf,
    path: PathBuf,
    added: bool,
}

impl Drop for Worktree {
    fn drop(&mut self) {
        // remove only the exact detached root created for this operation
        if self.added {
            let _ = command(
                &self.repository,
                &[
                    "worktree",
                    "remove",
                    "--force",
                    &self.path.to_string_lossy(),
                ],
                None,
            );
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Prepare a development-only model commit from verified publication material.
pub fn prepare_native_maintenance_model_release_commit(
    repository: &Path,
    publication: &NativeMaintenanceDevelopmentPublication,
) -> Result<NativeMaintenanceModelReleaseCommit, Error> {
    let action = &publication.action;
    let source = text(action, "expectedSourceCommit").ok_or(Error::Input)?;
    let family = text(action, "family").ok_or(Error::Input)?;
    let target = text(action, "candidateSha256").ok_or(Error::Input)?;
    let created = text(action, "createdAt").ok_or(Error::Input)?;
    // retain the original exact action identities
    if text(action, "actionKind").as_deref() != Some("shadow") {
        return Err(Error::Input);
    }
    prepare_commit(
        repository,
        CommitInput {
            source: &source,
            family: &family,
            target: &target,
            action_sha256: &publication.action_sha256,
            created: &created,
            files: &publication.package_files,
            removed: &[],
        },
    )
}

pub(crate) struct CommitInput<'a> {
    pub(crate) source: &'a str,
    pub(crate) family: &'a str,
    pub(crate) target: &'a str,
    pub(crate) action_sha256: &'a str,
    pub(crate) created: &'a str,
    pub(crate) files: &'a [NativeMaintenanceDevelopmentPublicationFile],
    pub(crate) removed: &'a [String],
}

/// Verify the retained production checkout, SSH agent and exact immutable source tag.
pub fn verify_native_maintenance_model_release_environment(
    repository: &Path,
    source_commit: &str,
    source_release: &str,
) -> Result<(), Error> {
    const ROOT: &str = "/home/ubuntu/weather";
    const SOCKET: &str = "/run/user/1000/openssh_agent";
    // require the exact fixed source identities before admitting Git process effects
    if repository != Path::new(ROOT) || !commit(source_commit) || !release_tag(source_release) {
        return Err(Error::Environment);
    }
    let root = fs::symlink_metadata(repository).map_err(|_| Error::Environment)?;
    let socket = fs::symlink_metadata(SOCKET).map_err(|_| Error::Environment)?;
    // same process owner and ordinary resolved production objects
    let uid = unsafe { libc::getuid() };
    if !root.is_dir()
        || root.uid() != uid
        || !socket.file_type().is_socket()
        || socket.uid() != uid
        || fs::canonicalize(repository).map_err(|_| Error::Environment)? != repository
        || fs::canonicalize(SOCKET).map_err(|_| Error::Environment)? != Path::new(SOCKET)
    {
        return Err(Error::Environment);
    }
    let keys = Command::new("/usr/bin/ssh-add")
        .arg("-L")
        .env("SSH_AUTH_SOCK", SOCKET)
        .output()
        .map_err(|_| Error::Environment)?;
    if !keys.status.success() || keys.stdout.len() > 64 * 1024 || keys.stderr.len() > 64 * 1024 {
        return Err(Error::Environment);
    }
    let keys = String::from_utf8(keys.stdout).map_err(|_| Error::Environment)?;
    // accept only the source's nonempty retained supported public-key listing
    if !keys.ends_with('\n')
        || keys.is_empty()
        || keys.split_terminator('\n').any(|line| {
            let mut fields = line.splitn(3, ' ');
            !matches!(fields.next(), Some("ssh-ed25519" | "ssh-rsa"))
                || fields.next().is_none_or(|key| {
                    key.is_empty()
                        || !key
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"+/=".contains(&byte))
                })
                || fields.next().is_some_and(|comment| comment.contains('\r'))
        })
    {
        return Err(Error::Environment);
    }
    if command(repository, &["remote", "get-url", "origin"], None)?
        != "git@github.com:anstosa/weather.git"
    {
        return Err(Error::Environment);
    }
    command(
        repository,
        &[
            "fetch",
            "--no-tags",
            "origin",
            &format!("refs/tags/{source_release}"),
        ],
        None,
    )?;
    if command(
        repository,
        &["rev-parse", "--verify", "FETCH_HEAD^{commit}"],
        None,
    )? != source_commit
    {
        return Err(Error::Environment);
    }
    Ok(())
}

/// Admit the source calendar release tag grammar.
fn release_tag(value: &str) -> bool {
    let Some((date, suffix)) = value.split_once('-') else {
        return false;
    };
    date.len() == 10
        && date.bytes().enumerate().all(|(i, byte)| {
            if i == 4 || i == 7 {
                byte == b'.'
            } else {
                byte.is_ascii_digit()
            }
        })
        && !suffix.is_empty()
        && suffix.len() <= 3
        && !suffix.starts_with('0')
        && suffix.bytes().all(|byte| byte.is_ascii_digit())
}

/// Construct the original package-only commit without advancing any branch.
pub(crate) fn prepare_commit(
    repository: &Path,
    input: CommitInput<'_>,
) -> Result<NativeMaintenanceModelReleaseCommit, Error> {
    // reject abbreviated commits and unknown action identities
    if !commit(input.source)
        || !matches!(input.family, "temperature" | "wind" | "rain")
        || !valid_hash(input.target)
        || !valid_hash(input.action_sha256)
        || input.files.is_empty()
    {
        return Err(Error::Input);
    }
    let mut allowed = BTreeSet::new();
    // admit only the exact package paths
    for file in input.files {
        if !package_path(&file.path) || !allowed.insert(file.path.as_str()) {
            return Err(Error::Package);
        }
    }
    // admit only the original optional control-reference selector removal
    for path in input.removed {
        if path != SELECTOR || !allowed.insert(path.as_str()) {
            return Err(Error::Package);
        }
    }
    let source = command(
        repository,
        &[
            "rev-parse",
            "--verify",
            &format!("{}^{{commit}}", input.source),
        ],
        None,
    )?;
    if source != input.source {
        return Err(Error::Input);
    }
    let root = temporary_root()?;
    let mut tree = Worktree {
        repository: repository.to_owned(),
        path: root.join("worktree"),
        root,
        added: false,
    };
    let path = tree.path.to_str().ok_or(Error::Worktree)?;
    command(
        repository,
        &["worktree", "add", "--detach", path, input.source],
        None,
    )?;
    tree.added = true;
    // ensure source worktree and submodules are exactly clean
    if !command(
        &tree.path,
        &["status", "--porcelain=v1", "--untracked-files=all"],
        None,
    )?
    .is_empty()
        || command(&tree.path, &["submodule", "status", "--recursive"], None)?
            .lines()
            .any(|line| !line.starts_with(' '))
    {
        return Err(Error::Worktree);
    }
    // materialize the admitted public package files
    for file in input.files {
        let path = regular_target(&tree.path, &file.path, true)?;
        fs::write(path, &file.bytes).map_err(|_| Error::Package)?;
    }
    // remove only admitted selector material that actually existed
    for name in input.removed {
        let path = regular_target(&tree.path, name, false)?;
        fs::remove_file(path).map_err(|_| Error::Package)?;
    }
    let changed = command(
        &tree.path,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        None,
    )?;
    // check the exact status path set before staging
    let actual: BTreeSet<_> = changed
        .split_terminator('\0')
        .map(|entry| entry.get(3..).unwrap_or(""))
        .collect();
    if actual != allowed || actual.len() != changed.split_terminator('\0').count() {
        return Err(Error::Package);
    }
    let mut args = vec!["add", "--"];
    args.extend(allowed.iter().copied());
    command(&tree.path, &args, None)?;
    let staged = command(&tree.path, &["diff", "--cached", "--name-only", "-z"], None)?;
    if staged.split_terminator('\0').collect::<BTreeSet<_>>() != allowed {
        return Err(Error::Package);
    }
    // reopen the staged exact bytes as well as the worktree files
    for file in input.files {
        let path = regular_target(&tree.path, &file.path, false)?;
        if fs::read(path).map_err(|_| Error::Package)? != file.bytes
            || raw(&tree.path, &["show", &format!(":{}", file.path)], None)? != file.bytes
        {
            return Err(Error::Package);
        }
    }
    // verify an admitted removal remains absent
    for name in input.removed {
        if fs::symlink_metadata(tree.path.join(name)).is_ok() {
            return Err(Error::Package);
        }
    }
    let target_prefix = &input.target[..12];
    let message = format!(
        "chore(adjustment): publish {} {target_prefix}",
        input.family
    );
    command(
        &tree.path,
        &[
            "-c",
            "user.name=Weather Adjustment Automation",
            "-c",
            "user.email=weather-adjustment@localhost",
            "commit",
            "--no-gpg-sign",
            "-m",
            &message,
        ],
        Some(input.created),
    )?;
    let commit_sha = command(&tree.path, &["rev-parse", "HEAD"], None)?;
    if !commit(&commit_sha) {
        return Err(Error::Git);
    }
    Ok(NativeMaintenanceModelReleaseCommit {
        commit_sha,
        branch: format!(
            "automation/adjustment-{}-{target_prefix}-{}",
            input.family,
            &input.action_sha256[..12]
        ),
        action_tag: format!("adjustment-action/{}/{}", input.family, input.action_sha256),
    })
}

/// Select the original bounded immutable date-tag namespace.
pub fn select_native_maintenance_model_release_tag(
    repository: &Path,
    date: &str,
    commit_sha: &str,
    first_suffix: u16,
) -> Result<String, Error> {
    // require the source calendar date format and exact commit
    if date.len() != 10
        || !date.bytes().enumerate().all(|(i, byte)| {
            if i == 4 || i == 7 {
                byte == b'.'
            } else {
                byte.is_ascii_digit()
            }
        })
        || !commit(commit_sha)
        || !(1..=999).contains(&first_suffix)
    {
        return Err(Error::Input);
    }
    // select only absent or idempotently matching refs
    for suffix in first_suffix..=999 {
        let tag = format!("{date}-{suffix}");
        let previous = remote_ref(repository, &format!("refs/tags/{tag}"))?;
        if previous.as_deref().is_none_or(|value| value == commit_sha) {
            return Ok(tag);
        }
    }
    Err(Error::Namespace)
}

/// Publish one source-owned immutable branch or tag without force.
pub fn publish_native_maintenance_model_release_ref(
    repository: &Path,
    commit_sha: &str,
    reference: &str,
) -> Result<(), Error> {
    // reject refspec operators and unrelated namespaces
    if !commit(commit_sha)
        || !reference
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/.-_".contains(&byte))
        || !(reference.starts_with("refs/heads/automation/adjustment-")
            || reference.starts_with("refs/tags/"))
    {
        return Err(Error::Input);
    }
    // accept exact retry and never overwrite an existing ref
    if let Some(previous) = remote_ref(repository, reference)? {
        return if previous == commit_sha {
            Ok(())
        } else {
            Err(Error::Collision)
        };
    }
    command(
        repository,
        &["push", "origin", &format!("{commit_sha}:{reference}")],
        None,
    )?;
    if remote_ref(repository, reference)?.as_deref() != Some(commit_sha) {
        return Err(Error::Collision);
    }
    Ok(())
}

/// Require one specific remote ref or its absence.
pub(crate) fn remote_ref(repository: &Path, reference: &str) -> Result<Option<String>, Error> {
    let output = command(
        repository,
        &["ls-remote", "--refs", "origin", reference],
        None,
    )?;
    if output.is_empty() {
        return Ok(None);
    }
    let (identity, name) = output.split_once('\t').ok_or(Error::Git)?;
    if name != reference || !commit(identity) {
        return Err(Error::Git);
    }
    Ok(Some(identity.to_owned()))
}

/// Validate an exact lowercase commit.
fn commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Restrict package paths to ordinary public forecast-adjustment paths.
fn package_path(value: &str) -> bool {
    value.starts_with("config/forecast-adjustments/")
        && value.len() <= 240
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/.-_".contains(&byte))
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && !value.contains("//")
        && !value.contains("..")
}

/// Return a regular package target beneath ordinary directories.
fn regular_target(root: &Path, value: &str, create: bool) -> Result<PathBuf, Error> {
    let mut target = root.to_owned();
    let mut components = Path::new(value).components().peekable();
    // inspect every path component before using it
    while let Some(component) = components.next() {
        target.push(component);
        let directory = components.peek().is_some();
        match fs::symlink_metadata(&target) {
            Ok(details) if (directory && details.is_dir()) || (!directory && details.is_file()) => {
            }
            Err(error) if create && error.kind() == ErrorKind::NotFound => {
                if directory {
                    fs::create_dir(&target).map_err(|_| Error::Package)?;
                }
            }
            _ => return Err(Error::Package),
        }
    }
    Ok(target)
}

/// Create a unique disposable release root owned by this process.
fn temporary_root() -> Result<PathBuf, Error> {
    // use atomic directory creation to avoid adopting an earlier root
    for _ in 0..32 {
        let path = std::env::temp_dir().join(format!(
            "weather-adjustment-release-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err(Error::Worktree),
        }
    }
    Err(Error::Worktree)
}

/// Run the original shell-free fixed Git executable.
fn raw(repository: &Path, args: &[&str], date: Option<&str>) -> Result<Vec<u8>, Error> {
    let mut process = Command::new("/usr/bin/git");
    process.current_dir(repository).args(args)
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_SSH_COMMAND", "/usr/bin/ssh -o BatchMode=yes -o IdentitiesOnly=yes -o IdentityAgent=/run/user/1000/openssh_agent -o StrictHostKeyChecking=yes -o UserKnownHostsFile=/home/ubuntu/.ssh/known_hosts")
        .env("GIT_TERMINAL_PROMPT", "0").env("HOME", "/home/ubuntu")
        .env("SSH_AUTH_SOCK", "/run/user/1000/openssh_agent");
    // bind source author and committer dates only for the prepared commit
    if let Some(date) = date {
        process
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date);
    }
    let output = process.output().map_err(|_| Error::Git)?;
    if !output.status.success()
        || output.stdout.len() > 4 * 1024 * 1024
        || output.stderr.len() > 4 * 1024 * 1024
    {
        return Err(Error::Git);
    }
    Ok(output.stdout)
}

/// Trim only the trailing source command newline.
fn command(repository: &Path, args: &[&str], date: Option<&str>) -> Result<String, Error> {
    let mut output = String::from_utf8(raw(repository, args, date)?).map_err(|_| Error::Git)?;
    if output.ends_with('\n') {
        output.pop();
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_prepared_commit_is_repeatable_and_leaves_source_untouched() {
        let root = temporary_root().unwrap();
        let fixture = Worktree {
            repository: root.clone(),
            root: root.clone(),
            path: root.clone(),
            added: false,
        };
        let repository = &fixture.path;
        let date = "2026-11-01T12:00:00.000Z";
        command(repository, &["init", "-q"], None).unwrap();
        fs::write(repository.join("README.md"), b"source\n").unwrap();
        command(repository, &["add", "README.md"], None).unwrap();
        command(
            repository,
            &[
                "-c",
                "user.name=Weather Adjustment Automation",
                "-c",
                "user.email=weather-adjustment@localhost",
                "commit",
                "--no-gpg-sign",
                "-m",
                "source",
            ],
            Some(date),
        )
        .unwrap();
        let source = command(repository, &["rev-parse", "HEAD"], None).unwrap();
        let target = "a".repeat(64);
        let action = "b".repeat(64);
        let files = [NativeMaintenanceDevelopmentPublicationFile {
            path: "config/forecast-adjustments/ballydidean/actions/sha256-fixture.json".to_owned(),
            bytes: b"{\"source\":true}".to_vec(),
        }];
        // construct the same source request twice to compare detached commit identity
        let input = || CommitInput {
            source: &source,
            family: "rain",
            target: &target,
            action_sha256: &action,
            created: date,
            files: &files,
            removed: &[],
        };
        let first = prepare_commit(repository, input()).unwrap();
        let second = prepare_commit(repository, input()).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first.branch,
            "automation/adjustment-rain-aaaaaaaaaaaa-bbbbbbbbbbbb"
        );
        assert_eq!(first.action_tag, format!("adjustment-action/rain/{action}"));
        assert_eq!(
            command(repository, &["rev-parse", "HEAD"], None).unwrap(),
            source
        );
        assert_eq!(
            raw(
                repository,
                &["show", &format!("{}:{}", first.commit_sha, files[0].path)],
                None
            )
            .unwrap(),
            files[0].bytes
        );
        assert!(
            command(repository, &["status", "--porcelain=v1"], None)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            command(repository, &["worktree", "list", "--porcelain"], None)
                .unwrap()
                .lines()
                .filter(|line| line.starts_with("worktree "))
                .count(),
            1
        );
    }
}
