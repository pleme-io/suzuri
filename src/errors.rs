pub type Result<T> = std::result::Result<T, SuzuriError>;

#[derive(Debug, thiserror::Error)]
pub enum SuzuriError {
    #[error("PTY error: {0}")]
    Pty(String),

    #[error("Renderer error: {0}")]
    Renderer(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Font error: {0}")]
    Font(String),

    #[error("Window error: {0}")]
    Window(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<shikumi::ShikumiError> for SuzuriError {
    fn from(err: shikumi::ShikumiError) -> Self {
        SuzuriError::Config(err.to_string())
    }
}
