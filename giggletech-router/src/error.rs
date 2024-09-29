/*
    error.rs - Error Handling for OSC Operations

    This module defines the `Error` enum to handle errors during OSC communication and a `Result<T>` 
    type alias for functions that may return these errors.

    **Key Features:**
    1. **Error Enum (`Error`)**:
       - Represents possible errors:
         - **IO error**: Related to input/output operations.
         - **OSC decode error**: Failure when decoding OSC packets.
       - Implements `From<rosc::OscError>` to allow easy conversion of OSC-specific errors.

    2. **Result Type Alias (`Result<T>`)**:
       - A type alias for `std::result::Result<T, Error>` to simplify function return types.

    **Usage**:
    - Use `Result<T>` in functions performing OSC operations for consistent error handling.
    - Propagate errors using the `?` operator for clean error handling.
*/

#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// IO error
    #[error("IO error")]
    Io(#[from] std::io::Error),
    /// OSC decode error
    #[error("Decode OSC packet failed")]
    Osc(rosc::OscError),
}

impl From<rosc::OscError> for Error {
    fn from(error: rosc::OscError) -> Self {
        Self::Osc(error)
    }
}

/// Result type for OSC operations.
pub type Result<T> = std::result::Result<T, Error>;
