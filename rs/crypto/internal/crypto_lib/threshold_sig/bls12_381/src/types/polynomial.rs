//! Polynomials over `Scalar`.
//!
//! Note: This file is largely based on https://github.com/poanetwork/threshold_crypto/blob/master/src/poly.rs

use ic_crypto_internal_bls12_381_type::Scalar;
use rand::{CryptoRng, RngCore};
use std::iter;
use zeroize::{Zeroize, ZeroizeOnDrop};

// Methods:
mod advanced_ops;
mod constructors;
mod ops;

#[cfg(test)]
pub mod arbitrary;
#[cfg(test)]
mod tests;

/// A univariate polynomial
/// Note: The polynomial terms are: coefficients[i] * x^i
///       E.g. 3 + 2x + x^2 - x^4 is encoded as:
///       Polynomial{ coefficients: [3,2,1,0,-1] }
#[derive(Clone, Debug, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct Polynomial {
    pub coefficients: Vec<Scalar>,
}
