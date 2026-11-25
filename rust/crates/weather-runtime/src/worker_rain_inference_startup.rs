//! Assemble the original rain sidecar from one installed serving selection.

use std::sync::Arc;

use crate::{
    ApiMaintenanceCandidateCodec, DatabaseRainInferenceBinding, HttpWorkerRainMaintenanceClient,
    LoadedWorkerRainServing, WorkerConfiguration, WorkerLoadedSidecarAdmission,
    WorkerRainControlReferenceFiles, WorkerRainControlRevision, WorkerRainFeatureRevisionCodec,
    WorkerRainInferenceServing, WorkerRainInferenceShadow, WorkerRainInferenceTask,
    WorkerRainMaintenanceHttpJson, WorkerRainMaintenanceOriginOwner, WorkerRainRevisionCodec,
    WorkerRainRevisionIdentity, WorkerRainServingFiles, WorkerRainServingValues,
    WorkerRainShadowStartup, load_worker_rain_control_reference, load_worker_rain_serving,
};

/// Private transport owners using one already configured worker data network.
pub struct WorkerRainArchiveHttp<Owner> {
    pub revision: Option<WorkerRainRevisionHttp<Owner>>,
    pub shadow: Owner,
}

/// Independent request owners on the same already configured private network.
pub struct WorkerRainRevisionHttp<Owner> {
    pub revision: Owner,
    pub control_state: Owner,
}

/// Per-process admitted rain primitive owners; not database or network connections.
pub struct WorkerRainInferenceStartup<Admission, Time, Evidence, Calendar, Owner> {
    pub admission: Admission,
    pub time: Time,
    pub evidence: Arc<Evidence>,
    pub calendar: Calendar,
    pub archive: Option<WorkerRainArchiveHttp<Owner>>,
}

/// Concrete scheduler task using the retained ingest pool and original archive contracts.
pub type StartedWorkerRainInference<Admission, Time, Evidence, Calendar, Owner> =
    WorkerRainInferenceTask<
        DatabaseRainInferenceBinding,
        WorkerRainInferenceServing<
            LoadedWorkerRainServing,
            Admission,
            Time,
            WorkerRainRevisionIdentity<Evidence>,
            WorkerRainRevisionCodec<Evidence, HttpWorkerRainMaintenanceClient<Owner>>,
            HttpWorkerRainMaintenanceClient<Owner>,
        >,
        WorkerRainInferenceShadow<Evidence, Calendar, HttpWorkerRainMaintenanceClient<Owner>>,
    >;

/// Bind selected inference, feature revision and shadow work to the existing sidecar admission.
pub fn prepare_worker_rain_inference<Admission, Time, Evidence, Calendar, Owner, Origin>(
    configuration: &WorkerConfiguration,
    loaded: &mut WorkerLoadedSidecarAdmission,
    files: &mut (impl WorkerRainServingFiles + WorkerRainControlReferenceFiles),
    codec: &mut impl ApiMaintenanceCandidateCodec,
    origin_owner: &mut Origin,
    startup: WorkerRainInferenceStartup<Admission, Time, Evidence, Calendar, Owner>,
) -> Result<StartedWorkerRainInference<Admission, Time, Evidence, Calendar, Owner>, Origin::Error>
where
    Evidence: WorkerRainServingValues,
    Owner: WorkerRainMaintenanceHttpJson,
    Origin: WorkerRainMaintenanceOriginOwner,
{
    let model = load_worker_rain_serving(files, codec, startup.evidence.as_ref());
    loaded.rain_adjustment_runtime = model.selection();
    loaded.rain_adjustment_revision_client_loaded = false;
    let mut revision = None;
    let mut shadow = None;
    // retain original no-origin and no-witness behavior without touching private transports
    if let (Some(origin), Some(epoch), Some(archive)) = (
        configuration.adjustment_maintenance_api_origin.as_deref(),
        loaded.adjustment_capture_epoch.clone(),
        startup.archive,
    ) {
        let shadow_client =
            HttpWorkerRainMaintenanceClient::admit(origin, origin_owner, archive.shadow)?;
        shadow = WorkerRainInferenceShadow::load(
            files,
            codec,
            WorkerRainShadowStartup {
                evidence: Arc::clone(&startup.evidence),
                calendar: startup.calendar,
                client: Some(shadow_client),
                epoch: Some(Arc::clone(&epoch)),
                authority: model.comparator_authority(),
            },
        );
        // independently resolve the v3 control slot, preserving feature-only on omission or load failure
        if let Some(owners) = archive.revision {
            let client =
                HttpWorkerRainMaintenanceClient::admit(origin, origin_owner, owners.revision)?;
            let control =
                match load_worker_rain_control_reference(files, codec, startup.evidence.as_ref())
                    .ok()
                    .flatten()
                {
                    Some(reference) => Some(WorkerRainControlRevision {
                        reference,
                        authority: model.comparator_authority(),
                        archive: HttpWorkerRainMaintenanceClient::admit(
                            origin,
                            origin_owner,
                            owners.control_state,
                        )?,
                    }),
                    None => None,
                };
            let features =
                WorkerRainFeatureRevisionCodec::new(Arc::clone(&startup.evidence), epoch);
            revision = Some((WorkerRainRevisionCodec::new(features, control), client));
        }
        loaded.rain_adjustment_revision_client_loaded = revision.is_some();
    }
    Ok(WorkerRainInferenceTask::new(
        DatabaseRainInferenceBinding,
        WorkerRainInferenceServing {
            admission: startup.admission,
            time: startup.time,
            identity: WorkerRainRevisionIdentity::new(startup.evidence),
            model,
            revision,
        },
        shadow,
    ))
}
