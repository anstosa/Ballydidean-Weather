/// supported RemoteAgents browser viewport selected by the admitted query parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteAgentsPreviewMode {
    Desktop,
    Mobile,
}

/// one root-relative preview destination normalized by the existing URI adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedRemoteAgentsPreviewTarget {
    location: String,
    mode: RemoteAgentsPreviewMode,
}

impl AdmittedRemoteAgentsPreviewTarget {
    /// admit one supported mode and same-origin normalized redirect target.
    pub fn admitted(mode: RemoteAgentsPreviewMode, location: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            mode,
        }
    }

    /// return the viewport mode proven by the URI adapter.
    pub const fn mode(&self) -> RemoteAgentsPreviewMode {
        self.mode
    }

    /// borrow the normalized root-relative redirect destination.
    pub fn location(&self) -> &str {
        &self.location
    }
}

/// exact transport-independent redirect selected by the preview bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAgentsPreviewResponse {
    pub cache_control: &'static str,
    pub content_length: usize,
    pub location: String,
    pub status_code: u16,
}

/// project one already-normalized preview target into the source redirect response.
pub fn handle_remote_agents_preview(
    target: AdmittedRemoteAgentsPreviewTarget,
) -> RemoteAgentsPreviewResponse {
    RemoteAgentsPreviewResponse {
        cache_control: "no-store",
        content_length: 0,
        location: target.location,
        status_code: 302,
    }
}
