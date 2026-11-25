use crate::AdmittedAdminOperator;

const SEVEN_DAYS_MS: i64 = 7 * 24 * 60 * 60 * 1_000;

/// one already-verified supported scorecard contract generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastAdjustmentScorecardContractVersion {
    V1,
    V2,
}

/// one admitted aggregate-only scorecard object before response wrapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedForecastAdjustmentScorecardJson(Vec<u8>);

impl AdmittedForecastAdjustmentScorecardJson {
    /// admit one verified compact scorecard object from the owning contract adapter.
    pub fn admitted(content: impl Into<Vec<u8>>) -> Self {
        Self(content.into())
    }

    /// borrow the admitted compact scorecard object.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// one hash- and contract-verified scorecard with admitted epoch boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedForecastAdjustmentScorecardSnapshot {
    contract_version: ForecastAdjustmentScorecardContractVersion,
    generated_at_epoch_ms: i64,
    scorecard: AdmittedForecastAdjustmentScorecardJson,
    valid_through_epoch_ms: i64,
}

impl AdmittedForecastAdjustmentScorecardSnapshot {
    /// admit one snapshot after hash, contract, and timestamp decoding succeeds.
    pub fn admitted(
        contract_version: ForecastAdjustmentScorecardContractVersion,
        generated_at_epoch_ms: i64,
        scorecard: AdmittedForecastAdjustmentScorecardJson,
        valid_through_epoch_ms: i64,
    ) -> Self {
        Self {
            contract_version,
            generated_at_epoch_ms,
            scorecard,
            valid_through_epoch_ms,
        }
    }

    /// return the admitted scorecard contract generation.
    pub const fn contract_version(&self) -> ForecastAdjustmentScorecardContractVersion {
        self.contract_version
    }

    /// borrow the admitted aggregate-only scorecard object.
    pub const fn scorecard(&self) -> &AdmittedForecastAdjustmentScorecardJson {
        &self.scorecard
    }
}

/// admitted publication slots after private storage and pointer verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastAdjustmentScorecardCandidates {
    pub current_v2: Option<AdmittedForecastAdjustmentScorecardSnapshot>,
    pub legacy: Option<AdmittedForecastAdjustmentScorecardSnapshot>,
    pub pending_v2: Option<AdmittedForecastAdjustmentScorecardSnapshot>,
    pub previous_v2_present: bool,
}

/// one source publication state exposed by the private admin response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastAdjustmentScorecardPublicationState {
    Current,
    LegacyDisplay,
    PendingUnapplied,
}

impl ForecastAdjustmentScorecardPublicationState {
    /// return the exact source publication state spelling.
    pub const fn wire_value(self) -> &'static str {
        // preserve the closed source slot names
        match self {
            Self::Current => "current",
            Self::LegacyDisplay => "legacy_display",
            Self::PendingUnapplied => "pending_unapplied",
        }
    }
}

/// one approved canonical clock observation at private scorecard read time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedForecastAdjustmentScorecardNow {
    pub canonical: String,
    pub epoch_ms: i64,
}

/// canonical scorecard read time retained outside this policy module.
pub trait ForecastAdjustmentScorecardClock {
    type Error;

    /// resolve one admitted canonical clock observation.
    fn now(&self) -> Result<AdmittedForecastAdjustmentScorecardNow, Self::Error>;
}

/// verified scorecard slots retained outside response and selection policy.
pub trait ForecastAdjustmentScorecardStore {
    type Error;

    /// read one coherent set of already-verified publication slots.
    fn read_candidates(
        &self,
        now: &AdmittedForecastAdjustmentScorecardNow,
    ) -> Result<ForecastAdjustmentScorecardCandidates, Self::Error>;
}

/// one complete response envelope admitted by the existing JSON encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedForecastAdjustmentScorecardResponseJson(Vec<u8>);

impl AdmittedForecastAdjustmentScorecardResponseJson {
    /// admit one compact data and publication-state response envelope.
    pub fn admitted(content: impl Into<Vec<u8>>) -> Self {
        Self(content.into())
    }

    /// borrow the admitted compact response envelope.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// response encoding retained in the canonical JSON adapter.
pub trait ForecastAdjustmentScorecardEncoder {
    type Error;

    /// wrap one verified scorecard in its selected publication state.
    fn encode_response(
        &self,
        snapshot: &AdmittedForecastAdjustmentScorecardSnapshot,
        publication_state: ForecastAdjustmentScorecardPublicationState,
    ) -> Result<AdmittedForecastAdjustmentScorecardResponseJson, Self::Error>;
}

/// preverified administrator authority or the absent session case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastAdjustmentScorecardAuthority<'a> {
    Admitted(&'a AdmittedAdminOperator),
    Missing,
}

/// one parsed private scorecard request method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastAdjustmentScorecardMethod {
    Get,
    Head,
    Unsupported,
}

/// one request after URL parsing and external session admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastAdjustmentScorecardRequest<'a> {
    pub authority: ForecastAdjustmentScorecardAuthority<'a>,
    pub has_query_delimiter: bool,
    pub method: ForecastAdjustmentScorecardMethod,
}

/// one private scorecard body retained for the owning protocol adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForecastAdjustmentScorecardBody {
    Json(Option<Vec<u8>>),
    Text(&'static [u8]),
}

/// exact response headers selected by the private scorecard path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastAdjustmentScorecardHeaders {
    pub allow: Option<&'static str>,
    pub cache_control: &'static str,
    pub content_length: usize,
    pub content_type: &'static str,
    pub retry_after: Option<&'static str>,
}

/// one complete private scorecard application response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastAdjustmentScorecardResponse {
    pub body: ForecastAdjustmentScorecardBody,
    pub headers: ForecastAdjustmentScorecardHeaders,
    pub status_code: u16,
}

/// serve one authenticated aggregate scorecard from verified publication slots.
pub fn handle_forecast_adjustment_scorecard<Clock, Store, Encoder>(
    request: ForecastAdjustmentScorecardRequest<'_>,
    clock: &Clock,
    store: &Store,
    encoder: &Encoder,
) -> ForecastAdjustmentScorecardResponse
where
    Clock: ForecastAdjustmentScorecardClock,
    Store: ForecastAdjustmentScorecardStore,
    Encoder: ForecastAdjustmentScorecardEncoder,
{
    // require external session admission before disclosing route availability
    let ForecastAdjustmentScorecardAuthority::Admitted(operator) = request.authority else {
        return scorecard_text_response(401, b"authentication required\n", "no-store", None, None);
    };
    let _verified_operator = operator;

    // reject caller-controlled publication selection before method handling
    if request.has_query_delimiter {
        return scorecard_text_response(400, b"bad request\n", "no-store", None, None);
    }

    // preserve the exact read-only method boundary
    let omit_body = match request.method {
        ForecastAdjustmentScorecardMethod::Get => false,
        ForecastAdjustmentScorecardMethod::Head => true,
        ForecastAdjustmentScorecardMethod::Unsupported => {
            return scorecard_text_response(
                405,
                b"method not allowed\n",
                "no-store",
                Some("GET, HEAD"),
                None,
            );
        }
    };
    // resolve one time observation only after authority and request admission
    let now = match clock.now() {
        Ok(now) => now,
        Err(_) => return scorecard_unavailable(),
    };
    // read one coherent verified slot view at that exact observation
    let candidates = match store.read_candidates(&now) {
        Ok(candidates) => candidates,
        Err(_) => return scorecard_unavailable(),
    };
    // close every absent, conflicting, stale, or mismatched publication
    let (publication_state, snapshot) = match select_scorecard(candidates, now.epoch_ms) {
        Some(selected) => selected,
        None => return scorecard_unavailable(),
    };
    // retain canonical response serialization in the existing encoder owner
    let encoded = match encoder.encode_response(&snapshot, publication_state) {
        Ok(encoded) => encoded,
        Err(_) => return scorecard_unavailable(),
    };

    // include the canonical JSON response newline without accepting overflow
    let Some(content_length) = encoded.as_bytes().len().checked_add(1) else {
        return scorecard_unavailable();
    };
    let mut content = encoded.0;
    content.push(b'\n');
    ForecastAdjustmentScorecardResponse {
        // omit only the successful HEAD response after complete selection and encoding
        body: ForecastAdjustmentScorecardBody::Json(if omit_body { None } else { Some(content) }),
        headers: ForecastAdjustmentScorecardHeaders {
            allow: None,
            cache_control: "private, no-store",
            content_length,
            content_type: "application/json; charset=utf-8",
            retry_after: None,
        },
        status_code: 200,
    }
}

/// select and validate the exact pending, current, or legacy publication source.
fn select_scorecard(
    candidates: ForecastAdjustmentScorecardCandidates,
    now_epoch_ms: i64,
) -> Option<(
    ForecastAdjustmentScorecardPublicationState,
    AdmittedForecastAdjustmentScorecardSnapshot,
)> {
    // reject forbidden legacy and selected-v2 coexistence
    if candidates.legacy.is_some()
        && (candidates.current_v2.is_some() || candidates.previous_v2_present)
    {
        return None;
    }
    // preserve pending, current, then legacy source priority
    let selected = if let Some(snapshot) = candidates.pending_v2 {
        (
            ForecastAdjustmentScorecardPublicationState::PendingUnapplied,
            snapshot,
        )
    } else if let Some(snapshot) = candidates.current_v2 {
        (
            ForecastAdjustmentScorecardPublicationState::Current,
            snapshot,
        )
    } else {
        let snapshot = candidates.legacy?;
        (
            ForecastAdjustmentScorecardPublicationState::LegacyDisplay,
            snapshot,
        )
    };
    // bind the selected slot class to its only accepted contract generation
    let expected_version = match selected.0 {
        ForecastAdjustmentScorecardPublicationState::LegacyDisplay => {
            ForecastAdjustmentScorecardContractVersion::V1
        }
        ForecastAdjustmentScorecardPublicationState::Current
        | ForecastAdjustmentScorecardPublicationState::PendingUnapplied => {
            ForecastAdjustmentScorecardContractVersion::V2
        }
    };

    // bind each selected slot to its admitted contract generation
    if selected.1.contract_version != expected_version {
        return None;
    }
    let validity_ms = selected
        .1
        .valid_through_epoch_ms
        .checked_sub(selected.1.generated_at_epoch_ms)?;

    // reject reversed, expired, and overlong v2 validity windows
    if validity_ms <= 0
        || selected.1.valid_through_epoch_ms <= now_epoch_ms
        || (selected.1.contract_version == ForecastAdjustmentScorecardContractVersion::V2
            && validity_ms > SEVEN_DAYS_MS)
    {
        return None;
    }
    Some(selected)
}

/// build one exact bounded text response without protocol I/O.
fn scorecard_text_response(
    status_code: u16,
    body: &'static [u8],
    cache_control: &'static str,
    allow: Option<&'static str>,
    retry_after: Option<&'static str>,
) -> ForecastAdjustmentScorecardResponse {
    ForecastAdjustmentScorecardResponse {
        body: ForecastAdjustmentScorecardBody::Text(body),
        headers: ForecastAdjustmentScorecardHeaders {
            allow,
            cache_control,
            content_length: body.len(),
            content_type: "text/plain; charset=utf-8",
            retry_after,
        },
        status_code,
    }
}

/// build the exact private unavailable response for every read failure.
fn scorecard_unavailable() -> ForecastAdjustmentScorecardResponse {
    scorecard_text_response(
        503,
        b"scorecard unavailable\n",
        "private, no-store",
        None,
        Some("300"),
    )
}
