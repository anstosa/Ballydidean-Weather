/// one retained source-scoped database session identity
pub trait RetainedSourceSession {
    /// return the immutable source identity held by the session
    fn source_id(&self) -> &str;
}
