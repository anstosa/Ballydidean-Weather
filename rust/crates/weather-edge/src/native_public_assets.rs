//! Standard-library file reader for the two release static-asset roots.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::StaticAssetRoot;
use crate::public_serving_application::{AdmittedPublicAssetBytes, PublicStaticAssetRead};

const PUBLIC_ROOT_COMPONENTS: [&str; 3] = ["apps", "web", "public"];
const COMPILED_ROOT_COMPONENTS: [&str; 3] = ["apps", "web", "dist"];

/// Read-only roots matching the deployed web server's public and compiled directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePublicAssetReader {
    compiled_root: PathBuf,
    public_root: PathBuf,
}

impl NativePublicAssetReader {
    /// bind explicitly supplied release roots without performing file access.
    pub fn new(public_root: impl Into<PathBuf>, compiled_root: impl Into<PathBuf>) -> Self {
        Self {
            compiled_root: compiled_root.into(),
            public_root: public_root.into(),
        }
    }

    /// derive the exact `apps/web/public` and `apps/web/dist` release roots.
    pub fn from_repository_root(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self::new(
            join_components(root, PUBLIC_ROOT_COMPONENTS),
            join_components(root, COMPILED_ROOT_COMPONENTS),
        )
    }

    /// borrow the selected fixed release root for startup wiring and diagnostics.
    pub fn root(&self, root: StaticAssetRoot) -> &Path {
        match root {
            StaticAssetRoot::Public => &self.public_root,
            StaticAssetRoot::Compiled => &self.compiled_root,
        }
    }
}

impl PublicStaticAssetRead for NativePublicAssetReader {
    type Error = io::Error;

    /// read one complete file after the serving plan selected its root and relative path.
    fn read_asset(
        &self,
        root: StaticAssetRoot,
        relative_path: &str,
    ) -> Result<AdmittedPublicAssetBytes, Self::Error> {
        let path = self.root(root).join(relative_path);
        fs::read(path).map(AdmittedPublicAssetBytes::admitted)
    }
}

/// append fixed source-owned components to one repository root.
fn join_components(root: &Path, components: [&str; 3]) -> PathBuf {
    let mut path = root.to_path_buf();
    // preserve the deployed source directory order
    for component in components {
        path.push(component);
    }
    path
}
