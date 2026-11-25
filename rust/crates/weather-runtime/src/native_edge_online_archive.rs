//! Owning process adapter for the existing borrowed native forecast scheduler archive.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io;
use std::path::{Path, PathBuf};

use weather_edge::{
    ForecastAllocationStatfsPort, ForecastArchiveAppendOutcome,
    ForecastCaptureCoordinatorInputError, ForecastCycleHashPair, ForecastEvidenceMaterial,
    ForecastEvidenceReceiptCodec, ForecastScheduledEvidenceArchive, ForecastSchedulerCalendarPort,
    NativeForecastSchedulerArchive, PreparedForecastEvidence, RevisionArchiveCanonicalClock,
};

/// A process-owned archive that reconstructs only borrowed operation-local storage owners.
///
/// The underlying archive graph deliberately borrows its material and calendar. Retaining those
/// borrows beside their owners would require a self-referential value, so this adapter owns the
/// long-lived inputs and opens the validated fixed-directory graph for each scheduled append.
pub struct NativeEdgeOnlineArchive<Codec, Calendar, Clock, Statfs> {
    root: PathBuf,
    material: ForecastEvidenceMaterial<Codec>,
    calendar: Calendar,
    clock: Clock,
    statfs: Statfs,
}

impl<Codec, Calendar, Clock, Statfs> NativeEdgeOnlineArchive<Codec, Calendar, Clock, Statfs>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Calendar: ForecastSchedulerCalendarPort,
    Clock: Clone + RevisionArchiveCanonicalClock,
    Statfs: Clone + ForecastAllocationStatfsPort,
{
    /// Validate the existing archive directories and retain every operation input by value.
    pub fn initialize(
        root: PathBuf,
        material: ForecastEvidenceMaterial<Codec>,
        calendar: Calendar,
        clock: Clock,
        statfs: Statfs,
    ) -> io::Result<Self> {
        NativeForecastSchedulerArchive::initialize(
            root.clone(),
            &material,
            &calendar,
            clock.clone(),
            statfs.clone(),
        )?;
        Ok(Self {
            root,
            material,
            calendar,
            clock,
            statfs,
        })
    }

    /// Borrow the one admitted evidence root shared by each operation-local archive graph.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Recover all supplied owners without opening or mutating archive storage.
    pub fn into_parts(
        self,
    ) -> (
        PathBuf,
        ForecastEvidenceMaterial<Codec>,
        Calendar,
        Clock,
        Statfs,
    ) {
        (
            self.root,
            self.material,
            self.calendar,
            self.clock,
            self.statfs,
        )
    }
}

impl<Codec, Calendar, Clock, Statfs> ForecastScheduledEvidenceArchive<PreparedForecastEvidence>
    for NativeEdgeOnlineArchive<Codec, Calendar, Clock, Statfs>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Calendar: ForecastSchedulerCalendarPort,
    Clock: Clone + RevisionArchiveCanonicalClock,
    Statfs: Clone + ForecastAllocationStatfsPort,
{
    type Error = NativeEdgeOnlineArchiveError;

    /// Settle one committed scheduler member through the complete existing archive coordinator.
    fn append_committed(
        &mut self,
        due_key: &str,
        prepared: &PreparedForecastEvidence,
    ) -> Result<ForecastArchiveAppendOutcome, Self::Error> {
        let mut archive = NativeForecastSchedulerArchive::initialize(
            self.root.clone(),
            &self.material,
            &self.calendar,
            self.clock.clone(),
            self.statfs.clone(),
        )
        .map_err(NativeEdgeOnlineArchiveError::Initialize)?;
        archive
            .append_committed(due_key, prepared)
            .map_err(NativeEdgeOnlineArchiveError::Input)
    }
}

/// Failures exposed before the existing coordinator can contain archive storage outcomes.
#[derive(Debug)]
pub enum NativeEdgeOnlineArchiveError {
    Initialize(io::Error),
    Input(ForecastCaptureCoordinatorInputError),
}

impl Display for NativeEdgeOnlineArchiveError {
    /// Render the bounded startup or coordinator-input refusal.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // retain initialization detail while keeping coordinator input categorical
        match self {
            Self::Initialize(error) => write!(formatter, "native online archive: {error}"),
            Self::Input(error) => write!(formatter, "native online archive input: {error:?}"),
        }
    }
}

impl Error for NativeEdgeOnlineArchiveError {
    /// Preserve only the concrete operating-system initialization cause.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // coordinator input errors are closed value refusals without a nested cause
        match self {
            Self::Initialize(error) => Some(error),
            Self::Input(_) => None,
        }
    }
}
