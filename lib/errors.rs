
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO Error {0}")]
    IO(#[from] std::io::Error),

    #[cfg(feature = "toml")]
    #[error("TOML Error {0}")]
    TOML(#[from] toml::de::Error),

    #[cfg(feature = "json")]
    #[error("Json Error {0}")]
    Json(#[from] serde_json::Error),

    #[error("IpAddr Error {0}")]
    IpAddr(#[from] std::net::AddrParseError),

    #[cfg(feature = "tls")]
    #[error("RcGen Error {0}")]
    RcGen(#[from] rcgen::Error),

    #[error("Not Supported")]
    NotSupported,

    #[error("Unknown Error")]
    Unknown,
}
