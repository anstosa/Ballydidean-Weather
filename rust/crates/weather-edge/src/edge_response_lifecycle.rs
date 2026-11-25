//! Attach post-response forecast evidence to the selected public edge application reply.

use crate::forecast_response_capture::prepare_forecast_response_capture_for_window;
use crate::{
    AdmittedForecastScheduleCapture, ForecastEvidenceWindow, ForecastPostResponseWork,
    ForecastResponseEvidenceStore, ForecastResponseTerminal, PendingForecastResponseCapture,
    RevisionArchiveCanonicalClock, WeatherEdgeDispatchOutcome, WeatherEdgeDispatchPortSet,
    WeatherEdgeDispatchRequest, dispatch_weather_edge_request, observe_forecast_response_terminal,
};

/// Snapshot the owning scheduler and its admitted association with this request.
pub struct EdgeForecastCaptureAdmission {
    pub channel_bound_capture_active: bool,
    pub scheduled_capture: Option<AdmittedForecastScheduleCapture>,
}

/// Hold only the evidence observer after the wire reply moves to the transport.
pub struct EdgeForecastTerminalObserver<Prepared>(Option<PendingForecastResponseCapture<Prepared>>);

impl<Prepared> EdgeForecastTerminalObserver<Prepared> {
    /// Determine whether the transport must register a terminal evidence observer.
    pub const fn is_pending(&self) -> bool {
        self.0.is_some()
    }

    /// Consume the first actual finish or close and return owned post-response work.
    pub fn observe<Store: ForecastResponseEvidenceStore<Prepared = Prepared>>(
        &mut self,
        terminal: ForecastResponseTerminal,
        clock: &mut impl RevisionArchiveCanonicalClock,
        store: &mut Store,
    ) -> Option<ForecastPostResponseWork<Prepared>> {
        observe_forecast_response_terminal(&mut self.0, terminal, clock, store)
    }
}

/// Derive the window from the same search sent upstream and capture only emitted bytes.
pub fn dispatch_edge_reply<Ports, Store>(
    request: WeatherEdgeDispatchRequest<'_>,
    ports: Ports,
    capture: EdgeForecastCaptureAdmission,
    normalize_window: impl FnOnce(&str) -> Option<ForecastEvidenceWindow>,
    store: &mut Store,
) -> (
    WeatherEdgeDispatchOutcome,
    EdgeForecastTerminalObserver<Store::Prepared>,
)
where
    Ports: WeatherEdgeDispatchPortSet,
    Store: ForecastResponseEvidenceStore,
{
    let admitted_search = request.search;
    let outcome = dispatch_weather_edge_request(request, ports);
    let pending_capture = match &outcome {
        // skip query decoding entirely for a non-forecast or unsuccessful response
        WeatherEdgeDispatchOutcome::Public(response)
            if response.track_successful_forecast_response =>
        {
            prepare_forecast_response_capture_for_window(
                response,
                None,
                normalize_window(admitted_search),
                capture.channel_bound_capture_active,
                capture.scheduled_capture,
                store,
            )
        }
        _ => None,
    };
    (outcome, EdgeForecastTerminalObserver(pending_capture))
}
