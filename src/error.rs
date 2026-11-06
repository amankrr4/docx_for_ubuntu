use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadError {
    NotAFile,
    Io(std::io::ErrorKind),
    NotZip,
    LegacyOrEncrypted,
    NotWordDocument,
    TooLarge,
    Malformed,
    Internal,
}

impl LoadError {
    pub fn message(self) -> &'static str {
        match self {
            Self::NotAFile | Self::Io(_) => {
                "Can't open this file. It may have been moved, or you may not have permission."
            }
            Self::NotZip => "This isn't a valid .docx file.",
            Self::LegacyOrEncrypted => {
                "This looks like an old .doc file or a password-protected document. DocxLite can't open it."
            }
            Self::NotWordDocument => "This file is a ZIP, but not a Word document.",
            Self::TooLarge => "This file is too large to open safely.",
            Self::Malformed => "This document is damaged. DocxLite couldn't read it.",
            Self::Internal => {
                "Something unexpected happened while reading this file. Your file was not changed."
            }
        }
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for LoadError {}

impl From<std::io::Error> for LoadError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.kind())
    }
}

pub type Result<T> = std::result::Result<T, LoadError>;
