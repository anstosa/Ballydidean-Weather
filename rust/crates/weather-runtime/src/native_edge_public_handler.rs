//! Actual native public request dispatch and terminal response settlement over retained route owners.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use hyper::{Method, Request, Response, body::Incoming};
use tokio::task::JoinHandle;
use weather_edge::{
    EdgeForecastCaptureAdmission, EdgeWireReply, ForecastResponseEvidenceStore,
    ForecastResponseTerminal, ForecastScheduledEvidenceArchive, RevisionArchiveCanonicalClock,
    XweatherMapProxyMethod, edge_terminal_admin_reply, edge_terminal_text_reply,
};

use crate::{
    NativeEdgeAdminAdmissionError, NativeEdgeAdminAdmissionOutcome, NativeEdgeAdminAdmissionOwner,
    NativeEdgeAdminBodyPlan, NativeEdgeAdminJson, NativeEdgeHttpRequest,
    NativeEdgeHttpResponseError, NativeEdgeOrdinaryAdmissionError,
    NativeEdgePostResponseExecutionError, NativeEdgePostResponseLane, NativeEdgePublicRouteGraph,
    NativeEdgeRouteExecution, NativeHttpConnectionScope, NativeProcessClock,
    NativeScheduledCaptureClaim, NativeScheduledCaptureRegistry, NativeTrackedHttpBody,
    NativeWirePrimitives, native_edge_http_response,
};

/// Retain the actual bounded route and post-response lanes used by all public sockets.
pub struct NativeEdgePublicHandler<Store: ForecastResponseEvidenceStore, Archive> {
    routes: NativeEdgeRouteExecution,
    admin: Arc<NativeEdgeAdminAdmissionOwner>,
    post_response: NativeEdgePostResponseLane<Store, Archive>,
    scheduler: NativeScheduledCaptureRegistry,
    release: Arc<str>,
    body_deadline: Duration,
    settlements: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl<Store, Archive> Clone for NativeEdgePublicHandler<Store, Archive>
where
    Store: ForecastResponseEvidenceStore + Clone,
{
    /// Share only the existing process owners across accepted sockets.
    fn clone(&self) -> Self {
        Self {
            routes: self.routes.clone(),
            admin: Arc::clone(&self.admin),
            post_response: self.post_response.clone(),
            scheduler: self.scheduler.clone(),
            release: Arc::clone(&self.release),
            body_deadline: self.body_deadline,
            settlements: Arc::clone(&self.settlements),
        }
    }
}

enum Dispatch<Prepared> {
    Application(EdgeWireReply<Prepared>),
    Terminal(EdgeWireReply<()>),
}

enum RequestStage<Prepared> {
    Complete(Dispatch<Prepared>),
    Body {
        request: Box<NativeEdgeHttpRequest>,
        plan: NativeEdgeAdminBodyPlan,
        capture: EdgeForecastCaptureAdmission,
    },
}

impl<Store, Archive> NativeEdgePublicHandler<Store, Archive>
where
    Store: ForecastResponseEvidenceStore + Clone + Send + 'static,
    Store::Prepared: Send + 'static,
    Archive: ForecastScheduledEvidenceArchive<Store::Prepared> + Send + 'static,
{
    /// Bind the real already initialized route graph and evidence owners to a socket handler.
    pub fn new(
        routes: NativeEdgeRouteExecution,
        admin: NativeEdgeAdminAdmissionOwner,
        post_response: NativeEdgePostResponseLane<Store, Archive>,
        scheduler: NativeScheduledCaptureRegistry,
        release: String,
        body_deadline: Duration,
    ) -> Self {
        Self {
            routes,
            admin: Arc::new(admin),
            post_response,
            scheduler,
            release: Arc::from(release),
            body_deadline,
            settlements: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Run the actual async body/synchronous admission/dispatch sequence over one real edge socket.
    pub async fn serve(
        &self,
        request: Request<Incoming>,
        peer: SocketAddr,
        scope: NativeHttpConnectionScope,
        channel_bound_capture_active: bool,
    ) -> Result<Response<NativeTrackedHttpBody>, NativeEdgeHttpResponseError> {
        let head = request.method() == Method::HEAD;
        let Ok(mut request) = NativeEdgeHttpRequest::read(request, peer, &NativeWirePrimitives)
        else {
            return untracked_response(internal_error(head), scope);
        };
        let claim = request.claim_scheduled_capture(&self.scheduler);
        let capture = EdgeForecastCaptureAdmission {
            channel_bound_capture_active,
            scheduled_capture: claim.as_ref().map(NativeScheduledCaptureClaim::slot),
        };
        let admin = Arc::clone(&self.admin);
        let release = Arc::clone(&self.release);
        let store = self.post_response.evidence_store();
        let stage = self
            .routes
            .execute(move |graph| {
                let outcome = admin.begin(&request, graph.admin());
                match outcome {
                    Ok(NativeEdgeAdminAdmissionOutcome::BodyRequired(plan)) => RequestStage::Body {
                        request: Box::new(request),
                        plan,
                        capture,
                    },
                    outcome => RequestStage::Complete(dispatch_request(
                        graph, &request, outcome, &release, capture, store,
                    )),
                }
            })
            .await;
        // never leave a real scheduler association waiting when route execution refuses the request
        let dispatch = match stage {
            Ok(RequestStage::Complete(dispatch)) => dispatch,
            Ok(RequestStage::Body {
                mut request,
                plan,
                capture,
            }) => {
                let body = match self
                    .admin
                    .read_body(&mut request, &plan, self.body_deadline)
                    .await
                {
                    Ok(body) => body,
                    Err(error) => return untracked_response(admin_error(error, head), scope),
                };
                let admin = Arc::clone(&self.admin);
                let release = Arc::clone(&self.release);
                let store = self.post_response.evidence_store();
                match self
                    .routes
                    .execute(move |graph| {
                        let outcome = admin.finish(&request, plan, body, graph.admin());
                        dispatch_request(graph, &request, outcome, &release, capture, store)
                    })
                    .await
                {
                    Ok(dispatch) => dispatch,
                    Err(_) => Dispatch::Terminal(internal_error(head)),
                }
            }
            Err(_) => Dispatch::Terminal(internal_error(head)),
        };
        match dispatch {
            Dispatch::Terminal(reply) => untracked_response(reply, scope),
            Dispatch::Application(reply) => {
                let lane = self.post_response.clone();
                let settlements = Arc::clone(&self.settlements);
                native_edge_http_response(reply, scope, move |observer, terminal| {
                    // sample the actual response terminal before queueing any blocking evidence work
                    let at = if terminal == ForecastResponseTerminal::Finished {
                        NativeProcessClock.canonical_now()
                    } else {
                        None
                    };
                    match lane.submit_terminal(observer, terminal, at) {
                        Ok(receipt) => {
                            // ordinary public captures need no per-request completion registration
                            if let Some(claim) = claim {
                                retain_completion(
                                    &settlements,
                                    tokio::spawn(async move {
                                        if let Ok(completed) = receipt.completion().await
                                            && let Some(result) = completed.scheduled
                                            && result.slot.due_key() == claim.slot().due_key()
                                        {
                                            claim.settle(result.settlement);
                                        } else {
                                            claim.fail();
                                        }
                                    }),
                                );
                            }
                        }
                        Err(_) => {
                            // surface the unaccepted terminal and fail its genuine scheduler claim
                            drop(claim);
                            let mut store = lane.evidence_store();
                            retain_completion(
                                &settlements,
                                tokio::spawn(async move {
                                    let _ = tokio::task::spawn_blocking(move || {
                                        store.note_write_failure()
                                    })
                                    .await;
                                }),
                            );
                        }
                    }
                })
            }
        }
    }

    /// Close route admissions after listener drain, finish all evidence work, then join scheduler settlements.
    pub async fn close_and_drain(&self) -> Result<(), NativeEdgePostResponseExecutionError> {
        self.routes.close_and_drain().await;
        let result = self.post_response.close_and_drain().await;
        let completions = std::mem::take(
            &mut *self
                .settlements
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
        // finish every retained receipt waiter after the evidence worker reached its actual terminal state
        for completion in completions {
            let _ = completion.await;
        }
        result
    }
}

/// Retain only live terminal waiter handles because completed ones cannot own further process work.
fn retain_completion(settlements: &Mutex<Vec<JoinHandle<()>>>, handle: JoinHandle<()>) {
    let mut tasks = settlements
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    tasks.retain(|task| !task.is_finished());
    tasks.push(handle);
}

/// Dispatch the complete admitted request on its native blocking worker.
fn dispatch_request<Store: ForecastResponseEvidenceStore>(
    graph: &NativeEdgePublicRouteGraph,
    request: &NativeEdgeHttpRequest,
    outcome: Result<NativeEdgeAdminAdmissionOutcome, NativeEdgeAdminAdmissionError>,
    release: &str,
    capture: EdgeForecastCaptureAdmission,
    mut store: Store,
) -> Dispatch<Store::Prepared> {
    let head = request.parts().method == Method::HEAD;
    let admission = match outcome {
        Ok(NativeEdgeAdminAdmissionOutcome::Admitted(admission)) => admission,
        Ok(NativeEdgeAdminAdmissionOutcome::Terminal(outcome)) => {
            return Dispatch::Terminal(edge_terminal_admin_reply(outcome, head));
        }
        Ok(NativeEdgeAdminAdmissionOutcome::BodyRequired(_)) => {
            return Dispatch::Terminal(internal_error(head));
        }
        Err(error) => return Dispatch::Terminal(admin_error(error, head)),
    };
    let mut admissions = admission.request_admissions();
    if let Err(error) = graph.admit_non_admin_routes(request, &mut admissions) {
        return Dispatch::Terminal(ordinary_error(error, head));
    }
    match request.dispatch(
        release,
        admissions,
        graph.ports(),
        capture,
        &mut store,
        &mut NativeEdgeAdminJson,
    ) {
        Ok(reply) => Dispatch::Application(reply),
        Err(_) => Dispatch::Terminal(internal_error(head)),
    }
}

/// Materialize exact administrator ingress failures through the ordinary source wire owner.
fn admin_error(error: NativeEdgeAdminAdmissionError, head: bool) -> EdgeWireReply<()> {
    edge_terminal_text_reply(error.status_code(), error.body(), None, head)
}

/// Preserve the source's explicit map-site or preview admission refusal behavior.
fn ordinary_error(error: NativeEdgeOrdinaryAdmissionError, head: bool) -> EdgeWireReply<()> {
    match error {
        NativeEdgeOrdinaryAdmissionError::InvalidPreview => {
            edge_terminal_text_reply(400, b"invalid browser preview\n", None, head)
        }
        NativeEdgeOrdinaryAdmissionError::MapSiteUnavailable(
            XweatherMapProxyMethod::Unsupported,
        ) => edge_terminal_text_reply(405, b"method not allowed\n", Some("GET, HEAD"), head),
        NativeEdgeOrdinaryAdmissionError::MapSiteUnavailable(_) => {
            edge_terminal_text_reply(404, b"not found\n", None, head)
        }
    }
}

/// Fail an unavailable request execution without inventing a successful response.
fn internal_error(head: bool) -> EdgeWireReply<()> {
    edge_terminal_text_reply(500, b"internal server error\n", None, head)
}

/// Use the same real native writer for a reply with no forecast observer.
fn untracked_response(
    reply: EdgeWireReply<()>,
    scope: NativeHttpConnectionScope,
) -> Result<Response<NativeTrackedHttpBody>, NativeEdgeHttpResponseError> {
    native_edge_http_response(reply, scope, |_, _| {})
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Check actual terminal route representations retain would-be HEAD content length.
    #[test]
    fn native_edge_public_handler_materializes_source_ingress_errors() {
        let preview = ordinary_error(NativeEdgeOrdinaryAdmissionError::InvalidPreview, true);
        assert_eq!(preview.response().status_code, 400);
        assert_eq!(
            preview.response().headers.content_length,
            b"invalid browser preview\n".len()
        );
        assert!(preview.response().body.is_empty());
        let method = ordinary_error(
            NativeEdgeOrdinaryAdmissionError::MapSiteUnavailable(
                XweatherMapProxyMethod::Unsupported,
            ),
            false,
        );
        assert_eq!(method.response().status_code, 405);
        assert_eq!(method.response().headers.allow, Some("GET, HEAD"));
    }
}
