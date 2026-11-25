//! Share the exact archive capture/export lease without involving public request serving.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;

use crate::native_revision_spool_file::{
    create_synced_file, invalid, lease_metadata, read_private_file, require_directory,
    sync_directory,
};

use super::{
    LOCK_ATTEMPTS, LOCK_DURATION_MILLISECONDS, LOCK_FILE_BYTES, LOCK_RETRY, retire_expired,
    uuid_from_linux_entropy, wall_milliseconds,
};

/// The existing canonical JSON/UTC owner for the archive transport lease record.
pub trait ForecastArchiveLockCodec {
    /// Encode the exact archive lock record with its random UUID and finite UTC expiry.
    fn encode_forecast_archive_lease(&self, expires_at: i64, token: &str) -> Option<Vec<u8>>;

    /// Recognize canonical retained bytes before trusting their declared expiry.
    fn decode_forecast_archive_expiry(&self, bytes: &[u8]) -> Option<i64>;
}

/// The admitted clock used for checking the same expiry under contention.
pub trait ForecastArchiveLockClock {
    /// Sample UTC milliseconds from the current runtime's admitted clock.
    fn now_epoch_milliseconds(&mut self) -> Option<i64>;
}

/// Keep acquisition and release faults distinct from the caller's operation.
#[derive(Debug)]
pub enum NativeForecastArchiveLeaseError<Operation> {
    Storage(io::Error),
    Busy,
    Operation(Operation),
}

/// Run one bounded native mutation under the Node-compatible archive transport lock.
pub fn with_native_forecast_archive_lease<T, Error>(
    evidence_root: &Path,
    codec: &impl ForecastArchiveLockCodec,
    clock: &mut impl ForecastArchiveLockClock,
    operation: impl FnOnce() -> Result<T, Error>,
) -> Result<T, NativeForecastArchiveLeaseError<Error>> {
    use NativeForecastArchiveLeaseError as ErrorKind;

    let lease = match ArchiveLease::acquire(evidence_root, codec, clock) {
        Ok(lease) => lease,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Err(ErrorKind::Busy),
        Err(error) => return Err(ErrorKind::Storage(error)),
    };
    let result = operation();
    lease.finish().map_err(ErrorKind::Storage)?;
    result.map_err(ErrorKind::Operation)
}

/// Own one identity-checked release, including unwinding outside the normal return path.
struct ArchiveLease {
    directory: PathBuf,
    path: PathBuf,
    bytes: Vec<u8>,
    released: bool,
}

impl ArchiveLease {
    /// Acquire only the Node archive transport lock, with its fixed retry and lease limits.
    fn acquire(
        root: &Path,
        codec: &impl ForecastArchiveLockCodec,
        clock: &mut impl ForecastArchiveLockClock,
    ) -> io::Result<Self> {
        require_directory(root)?;
        let pages = root.join("online-pages");
        require_directory(&pages)?;
        let directory = pages.join("locks");
        require_directory(&directory)?;
        let path = directory.join("transport.lock");
        let now = sampled_epoch(clock)?;
        let expires = now.checked_add(LOCK_DURATION_MILLISECONDS).ok_or_else(
            // keep the finite deadline exact even if the admitted clock overflows
            || invalid("archive transport lock expiry overflows"),
        )?;
        let token = uuid_from_linux_entropy()?;
        let bytes = codec
            .encode_forecast_archive_lease(expires, &token)
            .ok_or_else(|| invalid("archive transport lease could not be encoded"))?;
        // never publish unreadable lease bytes into the shared transport directory
        if bytes.len() > LOCK_FILE_BYTES
            || codec.decode_forecast_archive_expiry(&bytes) != Some(expires)
        {
            return Err(invalid(
                "archive transport lock expiry differs from its encoding",
            ));
        }
        // bounded waiting is exclusive to the already isolated archive lane
        for _ in 0..LOCK_ATTEMPTS {
            match create_synced_file(&path, &bytes) {
                Ok(()) => {
                    sync_directory(&directory)?;
                    return Ok(Self {
                        directory,
                        path,
                        bytes,
                        released: false,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    // retire only an expired canonical lease or a full-duration leftover file
                    if archive_lease_expired(&path, sampled_epoch(clock)?, codec)? {
                        retire_expired(&directory, &path)?;
                        continue;
                    }
                    thread::sleep(LOCK_RETRY);
                }
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "archive transport is busy",
        ))
    }

    /// Surface a filesystem error only when this lease still owns the retained lock.
    fn finish(mut self) -> io::Result<()> {
        self.released = true;
        self.release()
    }

    /// Release the retained lock only if its exact written identity still matches.
    fn release(&self) -> io::Result<()> {
        // a missing or unreadable lock does not authorize touching another holder
        if read_private_file(&self.path, LOCK_FILE_BYTES)
            .ok()
            .as_deref()
            == Some(self.bytes.as_slice())
        {
            fs::remove_file(&self.path)?;
            sync_directory(&self.directory)?;
        }
        Ok(())
    }
}

impl Drop for ArchiveLease {
    /// Clean up the exact owned lock after unwinding, without masking the original fault.
    fn drop(&mut self) {
        // avoid a second attempt after the explicit finish path
        if !self.released {
            let _ = self.release();
        }
    }
}

/// Trust a complete codec-validated expiry before falling back to crash-left file age.
fn archive_lease_expired(
    path: &Path,
    now: i64,
    codec: &impl ForecastArchiveLockCodec,
) -> io::Result<bool> {
    // accept only a complete recognized document before the predecessor's age fallback
    if let Ok(bytes) = read_private_file(path, LOCK_FILE_BYTES)
        && let Some(expiry) = codec.decode_forecast_archive_expiry(&bytes)
    {
        return Ok(expiry <= now);
    }
    let modified = wall_milliseconds(lease_metadata(path)?.modified()?)?;
    Ok(modified.saturating_add(LOCK_DURATION_MILLISECONDS) <= now)
}

/// Do not invent the archive lease's UTC observation after a clock refusal.
fn sampled_epoch(clock: &mut impl ForecastArchiveLockClock) -> io::Result<i64> {
    clock
        .now_epoch_milliseconds()
        .ok_or_else(|| invalid("archive transport lock clock is unavailable"))
}
