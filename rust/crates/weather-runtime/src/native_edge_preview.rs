//! Native same-origin RemoteAgents browser preview admission.

use url::Url;
use weather_edge::{AdmittedRemoteAgentsPreviewTarget, RemoteAgentsPreviewMode};

use crate::DecodedNativeEdgeTarget;

/// Existing browser preview query admission refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeEdgePreviewInvalid;

/// Normalize the first source mode/location pair through the approved URL primitive.
pub fn admit_native_edge_preview(
    target: &DecodedNativeEdgeTarget,
) -> Result<AdmittedRemoteAgentsPreviewTarget, NativeEdgePreviewInvalid> {
    let mode = match query_first(target, "mode") {
        Some("desktop") => RemoteAgentsPreviewMode::Desktop,
        Some("mobile") => RemoteAgentsPreviewMode::Mobile,
        _ => return Err(NativeEdgePreviewInvalid),
    };
    let location = query_first(target, "location").ok_or(NativeEdgePreviewInvalid)?;
    // require the source's root-relative location contract
    if !location.starts_with('/') || location.starts_with("//") {
        return Err(NativeEdgePreviewInvalid);
    }
    let base = Url::parse("http://weather.invalid").expect("fixed preview origin");
    let destination = base.join(location).map_err(|_| NativeEdgePreviewInvalid)?;
    // retain only the normalized same-origin representation selected by the source
    if destination.origin() != base.origin() {
        return Err(NativeEdgePreviewInvalid);
    }
    let mut normalized = String::from(destination.path());
    // Node URL.search omits an empty query delimiter
    if let Some(query) = destination.query().filter(|query| !query.is_empty()) {
        normalized.push('?');
        normalized.push_str(query);
    }
    // Node URL.hash omits an empty fragment delimiter
    if let Some(fragment) = destination
        .fragment()
        .filter(|fragment| !fragment.is_empty())
    {
        normalized.push('#');
        normalized.push_str(fragment);
    }
    Ok(AdmittedRemoteAgentsPreviewTarget::admitted(
        mode, normalized,
    ))
}

/// Preserve URLSearchParams.get's first-value behavior.
fn query_first<'a>(target: &'a DecodedNativeEdgeTarget, key: &str) -> Option<&'a str> {
    target
        .query
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preserve normal browser preview mode and a normalized local route with query and fragment.
    #[test]
    fn native_edge_preview_normalizes_ordinary_browser_route() {
        let target = DecodedNativeEdgeTarget {
            pathname: "/__rac/browser-device".to_owned(),
            search: String::new(),
            query: vec![
                ("mode".to_owned(), "mobile".to_owned()),
                (
                    "location".to_owned(),
                    "/weather/../forecast?days=1#hours".to_owned(),
                ),
            ],
        };
        let result = admit_native_edge_preview(&target).unwrap();
        assert_eq!(result.mode(), RemoteAgentsPreviewMode::Mobile);
        assert_eq!(result.location(), "/forecast?days=1#hours");
    }
}
