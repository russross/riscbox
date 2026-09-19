//! Host-provided cryptographic entropy for boot seeding and guest devices.

use core::fmt;
use std::cell::RefCell;
use std::io::Read;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntropyError;

impl fmt::Display for EntropyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("host entropy source failed")
    }
}

impl std::error::Error for EntropyError {}

pub trait EntropySource {
    /// Fills the complete destination with cryptographic entropy.
    ///
    /// # Errors
    ///
    /// Returns an error when the host cannot supply the requested bytes.
    fn fill(&mut self, destination: &mut [u8]) -> Result<(), EntropyError>;
}

pub type SharedEntropy = Rc<RefCell<dyn EntropySource>>;

pub struct SystemEntropy(std::fs::File);

impl SystemEntropy {
    /// Opens the native operating-system entropy source.
    ///
    /// # Errors
    ///
    /// Returns an error when the operating-system source is unavailable.
    pub fn open() -> Result<Self, EntropyError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::fs::File::open("/dev/urandom")
                .map(Self)
                .map_err(|_| EntropyError)
        }
        #[cfg(target_arch = "wasm32")]
        {
            Err(EntropyError)
        }
    }
}

impl EntropySource for SystemEntropy {
    fn fill(&mut self, destination: &mut [u8]) -> Result<(), EntropyError> {
        self.0.read_exact(destination).map_err(|_| EntropyError)
    }
}
