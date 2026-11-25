//! Connect retained worker scheduling to serving inference and optional shadow publication.

use std::sync::Arc;

use weather_api::AdmittedAdjustmentRevisionCaptureWitness;
use weather_contracts::MaintenanceShadowServingAuthority;
use weather_database::{ApiMaintenancePredictionValues, RainMaintenancePredictionWritePort};
use weather_worker::{
    RainAdjustmentClient, RainAdjustmentEvaluation, RainAdjustmentExecution,
    RainAdjustmentFeatureAdmission, RainAdjustmentIdentityAdapter, RainAdjustmentModel,
    RainAdjustmentPendingInput, RainAdjustmentProjectionCodec, RainAdjustmentRepository,
    RainAdjustmentRevisionExecution, RainAdjustmentRuntimeSelection, RainAdjustmentTimeAdapter,
    RainPortableReplayCalendar, TaskFailure, publish_rain_adjustment,
    publish_rain_adjustment_with_shadow,
};

use crate::{
    ApiMaintenanceCandidateCodec, ApiMaintenanceShadowEvidenceValues,
    LoadedWorkerRainMaintenanceCandidate, LoadedWorkerRainServing, RainMaintenanceCandidateFiles,
    RetainedWorkerSidecarTask, WorkerRainShadowInput, WorkerRainShadowPublicationClient,
    load_worker_rain_maintenance_candidate, run_worker_rain_shadow,
};

const CYCLE_MILLISECONDS: i64 = 6 * 3_600_000;
const DUE_OFFSET_MILLISECONDS: i64 = 35 * 60_000;

/// Bind the serving repository only for the length of one inference append.
pub trait WorkerRainInferenceRepositoryBinding<Database, Values> {
    type Repository<'a>: RainAdjustmentRepository
    where
        Self: 'a,
        Database: 'a,
        Values: 'a;

    /// Borrow the same pool and value owner as the other scheduled work.
    fn bind<'a>(
        &'a mut self,
        database: &'a mut Database,
        values: &'a mut Values,
    ) -> Self::Repository<'a>;
}

/// A retained model paired with the runtime selection that authorizes its use.
pub trait WorkerRainInferenceModel {
    /// Borrow an admitted model and its serving mode without reparsing the model.
    fn serving_model(&self) -> Option<(RainAdjustmentRuntimeSelection, RainAdjustmentModel<'_>)>;
}

impl WorkerRainInferenceModel for LoadedWorkerRainServing {
    /// Share the startup-bound model and exact selected mode.
    fn serving_model(&self) -> Option<(RainAdjustmentRuntimeSelection, RainAdjustmentModel<'_>)> {
        Some((self.selection(), self.model()?))
    }
}

/// The retained serving model and primitive owners of one worker installation.
pub struct WorkerRainInferenceServing<Model, Admission, Time, Identity, Codec, Client> {
    pub admission: Admission,
    pub time: Time,
    pub identity: Identity,
    pub model: Model,
    pub revision: Option<(Codec, Client)>,
}

/// Optional work using the settled serving input without changing its result.
pub trait WorkerRainAfterServing<Database, Values> {
    /// Reborrow the retained pool only after serving and its revision have finished.
    fn run_shadow(
        &mut self,
        pending: &RainAdjustmentPendingInput,
        evaluation: &RainAdjustmentEvaluation,
        database: &mut Database,
        values: &mut Values,
    );
}

/// The concrete inference sidecar for the retained worker coordinator.
pub struct WorkerRainInferenceTask<Binding, Serving, Shadow> {
    binding: Binding,
    serving: Serving,
    shadow: Option<Shadow>,
}

impl<Binding, Serving, Shadow> WorkerRainInferenceTask<Binding, Serving, Shadow> {
    /// Retain admitted serving owners and an independently optional shadow task.
    pub const fn new(binding: Binding, serving: Serving, shadow: Option<Shadow>) -> Self {
        Self {
            binding,
            serving,
            shadow,
        }
    }
}

impl<Database, Values, Binding, Model, Admission, Time, Identity, Codec, Client, Shadow>
    RetainedWorkerSidecarTask<Database, Values, i64>
    for WorkerRainInferenceTask<
        Binding,
        WorkerRainInferenceServing<Model, Admission, Time, Identity, Codec, Client>,
        Shadow,
    >
where
    Binding: WorkerRainInferenceRepositoryBinding<Database, Values>,
    Model: WorkerRainInferenceModel,
    Admission: RainAdjustmentFeatureAdmission,
    Time: RainAdjustmentTimeAdapter,
    Identity: RainAdjustmentIdentityAdapter,
    Codec: RainAdjustmentProjectionCodec,
    Client: RainAdjustmentClient,
    Shadow: WorkerRainAfterServing<Database, Values>,
{
    type Output = bool;

    /// Execute the exact settled-serving then best-effort shadow sequence.
    fn run(
        &mut self,
        now_ms: i64,
        database: &mut Database,
        values: &mut Values,
    ) -> Result<bool, TaskFailure> {
        let serving = &mut self.serving;
        // keep a disabled or failed startup selection off the database path
        let Some((runtime, model)) = serving.model.serving_model() else {
            return Ok(false);
        };
        // when shadow is inactive avoid retaining the completed evaluation across reborrows
        let mut completed = None;
        let published = {
            let mut repository = self.binding.bind(database, values);
            let mut execution = RainAdjustmentExecution {
                admission: &mut serving.admission,
                identity: &mut serving.identity,
                model,
                repository: &mut repository,
                revision: serving
                    .revision
                    .as_mut()
                    .map(|(codec, client)| RainAdjustmentRevisionExecution { codec, client }),
                runtime,
                time: &mut serving.time,
            };
            // keep the no-shadow path identical to ordinary worker inference
            if self.shadow.is_some() {
                publish_rain_adjustment_with_shadow(
                    now_ms,
                    &mut execution,
                    |pending, evaluation| {
                        completed = Some((pending, evaluation));
                    },
                )
            } else {
                publish_rain_adjustment(now_ms, &mut execution)
            }
        }?;
        // the serving repository and its session no longer borrow the retained resources
        if let (Some(shadow), Some((pending, evaluation))) = (&mut self.shadow, completed) {
            shadow.run_shadow(&pending, &evaluation, database, values);
        }
        Ok(published)
    }
}

/// Loaded private relay, approved evidence primitives and immutable serving witness.
pub struct WorkerRainShadowStartup<Evidence, Calendar, Client> {
    pub evidence: Arc<Evidence>,
    pub calendar: Calendar,
    pub client: Option<Client>,
    pub epoch: Option<Arc<AdmittedAdjustmentRevisionCaptureWitness>>,
    pub authority: Option<Arc<MaintenanceShadowServingAuthority>>,
}

/// Installed rain shadow work attached to the scheduled serving inference.
pub struct WorkerRainInferenceShadow<Evidence, Calendar, Client> {
    candidate: LoadedWorkerRainMaintenanceCandidate,
    evidence: Arc<Evidence>,
    calendar: Calendar,
    client: Client,
    epoch: Arc<AdmittedAdjustmentRevisionCaptureWitness>,
    authority: Option<Arc<MaintenanceShadowServingAuthority>>,
}

impl<Evidence, Calendar, Client> WorkerRainInferenceShadow<Evidence, Calendar, Client>
where
    Evidence: ApiMaintenanceShadowEvidenceValues,
{
    /// Load only with the private relay and witness; absent or bad material never stops serving.
    pub fn load(
        files: &mut impl RainMaintenanceCandidateFiles,
        codec: &mut impl ApiMaintenanceCandidateCodec,
        startup: WorkerRainShadowStartup<Evidence, Calendar, Client>,
    ) -> Option<Self> {
        let client = startup.client?;
        let epoch = startup.epoch?;
        let candidate =
            load_worker_rain_maintenance_candidate(files, codec, startup.evidence.as_ref())
                .ok()
                .flatten()?;
        Some(Self {
            candidate,
            evidence: startup.evidence,
            calendar: startup.calendar,
            client,
            epoch,
            authority: startup.authority,
        })
    }
}

impl<Database, Values, Evidence, Calendar, Client> WorkerRainAfterServing<Database, Values>
    for WorkerRainInferenceShadow<Evidence, Calendar, Client>
where
    Database: RainMaintenancePredictionWritePort,
    Values: ApiMaintenancePredictionValues + RainAdjustmentTimeAdapter,
    Evidence: ApiMaintenanceShadowEvidenceValues,
    Calendar: RainPortableReplayCalendar,
    Client: WorkerRainShadowPublicationClient,
{
    /// Derive the original due key from the retained forecast cycle and publish best-effort.
    fn run_shadow(
        &mut self,
        pending: &RainAdjustmentPendingInput,
        evaluation: &RainAdjustmentEvaluation,
        database: &mut Database,
        values: &mut Values,
    ) {
        // preserve invalid-cycle suppression before sending a private stage or gap
        let Some(due_key) = shadow_due_key(self.evidence.as_ref(), values, evaluation) else {
            return;
        };
        let _ = run_worker_rain_shadow(
            self.evidence.as_ref(),
            WorkerRainShadowInput {
                pending,
                evaluation,
                candidate: &self.candidate,
                epoch: &self.epoch,
                authority: self.authority.as_deref(),
                due_key: &due_key,
            },
            &mut self.calendar,
            database,
            values,
            &mut self.client,
        );
    }
}

/// Reproduce the source-owned six-hour due identity without consulting another clock.
fn shadow_due_key(
    evidence: &impl ApiMaintenanceShadowEvidenceValues,
    time: &mut impl RainAdjustmentTimeAdapter,
    evaluation: &RainAdjustmentEvaluation,
) -> Option<String> {
    let initialized =
        evidence.canonical_utc_epoch_milliseconds(&evaluation.run.run_initialized_at)?;
    // reject off-cycle forecasts exactly as the original worker does
    if initialized.rem_euclid(CYCLE_MILLISECONDS) != 0 {
        return None;
    }
    let due = time
        .utc_string(initialized.checked_add(DUE_OFFSET_MILLISECONDS)?)
        .ok()?;
    Some(format!("capture/{due}"))
}
