//! Bind the five private archive owners to one previously admitted native root.

use std::io;
use std::path::PathBuf;

use crate::{NativeShadowProducerCodec, NativeShadowProducerPrimitives};
use weather_edge::{
    ArchiveWireCodec, NativeRevisionSpool, PermanentArchiveGapStore, PrivateArchiveStorage,
    PrivateArchiveStores, PrivateRevisionArchive, PrivateShadowArchive, RainControlStateArchive,
    RainFixedGaugeGapArchive, RevisionArchiveCanonicalClock,
};

/// Compose the actual native listener stores without creating or changing the archive root.
///
/// Each handle uses the same root and cross-process lease; the supplied primitive and clock
/// handles are cloned rather than creating another runtime or filesystem custody owner.
pub fn bind_native_private_archive_stores<P, Clock>(
    root: PathBuf,
    primitives: P,
    clock: Clock,
) -> io::Result<impl PrivateArchiveStorage>
where
    P: NativeShadowProducerPrimitives + Clone,
    Clock: RevisionArchiveCanonicalClock + Clone,
{
    // admit the same existing native spool for every independent route owner
    let spool =
        || NativeRevisionSpool::new(root.clone(), ArchiveWireCodec::new(primitives.clone()));
    Ok(PrivateArchiveStores {
        rain_control: RainControlStateArchive::new(
            spool()?,
            ArchiveWireCodec::new(primitives.clone()),
            clock.clone(),
        ),
        fixed_gauge_gap: RainFixedGaugeGapArchive::new(
            spool()?,
            ArchiveWireCodec::new(primitives.clone()),
            clock.clone(),
        ),
        shadow: PrivateShadowArchive::new(
            spool()?,
            NativeShadowProducerCodec::new(primitives.clone()),
            clock.clone(),
        ),
        revision: PrivateRevisionArchive::new(
            spool()?,
            ArchiveWireCodec::new(primitives.clone()),
            clock.clone(),
        ),
        permanent_gap: PermanentArchiveGapStore::new(
            spool()?,
            ArchiveWireCodec::new(primitives),
            clock,
        ),
    })
}
