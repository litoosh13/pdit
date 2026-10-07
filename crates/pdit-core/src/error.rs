use std::fmt;

/// Errors returned by pdit-core.
#[derive(Debug)]
pub enum Error {
    /// PDFium reported an error.
    Pdfium(String),
    /// The text to replace was not found in the document.
    TextNotFound(String),
    /// Neither the original font nor the fallback font can show the text.
    /// Nothing was saved.
    UnsupportedCharacters {
        intended: String,
        reads_back_as: String,
    },
    /// The browser engine could not be started.
    EngineStart(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Pdfium(message) => write!(f, "PDF engine error: {message}"),
            Error::TextNotFound(text) => write!(f, "text not found: {text:?}"),
            Error::UnsupportedCharacters {
                intended,
                reads_back_as,
            } => write!(
                f,
                "no available font can show {intended:?} (reads back as {reads_back_as:?}); not saved"
            ),
            Error::EngineStart(message) => write!(f, "could not start the PDF engine: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<pdfium_render::prelude::PdfiumError> for Error {
    fn from(error: pdfium_render::prelude::PdfiumError) -> Self {
        Error::Pdfium(format!("{error:?}"))
    }
}
