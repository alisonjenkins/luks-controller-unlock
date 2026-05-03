use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),

    #[error("PIN exceeds maximum length of {max} buttons")]
    PinTooLong { max: usize },

    #[error("PIN is empty")]
    PinEmpty,

    #[error("PIN confirmation did not match")]
    PinMismatch,

    #[error("evdev code does not map to a canonical button")]
    #[allow(dead_code)]
    UnknownButton,

    #[error("no DRM card with a connected output")]
    NoDrmConnector,

    #[error("no controller detected")]
    NoController,

    #[error("ask-password file malformed: {0}")]
    AskPasswordParse(String),

    #[error("ask-password reply socket: {0}")]
    AskPasswordReply(String),

    #[error("cryptsetup failed: {0}")]
    Cryptsetup(String),

    #[error("subprocess: {0}")]
    Subprocess(String),

    #[error("unsupported initrd configuration: {0}")]
    Unsupported(String),

    #[error("DRM: {0}")]
    Drm(String),

    #[error("evdev: {0}")]
    Evdev(String),

    #[error("invalid configuration: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, Error>;
