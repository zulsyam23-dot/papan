//! Core shared types for the PAPAN toolchain.
//!
//! Everything in this crate is deterministic and serializable. Lengths are
//! stored as signed integer nanometres; angles as millidegrees. This keeps the
//! compiler and geometry kernels independent from unstable floating point
//! rounding when comparing or hashing design objects.

pub mod diagnostic;
pub mod id;
pub mod quantity;
pub mod source;
pub mod units;

pub use diagnostic::{Diagnostic, Diagnostics, Severity, ValidationReport};
pub use id::ObjectId;
pub use quantity::{Quantity, UnitError, UnitKind};
pub use source::Span;
pub use units::{Angle, Length, Point};
