//! Threshold signatures with a simple dealing mechanism.

use super::types::{
    CombinedSignature, CombinedSignatureBytes, IndividualSignature, IndividualSignatureBytes,
    Polynomial, PublicCoefficients, SecretKey, Signature,
};
use crate::api::dkg_errors::InvalidArgumentError;

use crate::types::PublicKey;
use ic_crypto_internal_bls12_381_type::{verify_bls_signature, G1Projective, G2Affine, Scalar};
use ic_crypto_internal_seed::Seed;
use ic_crypto_internal_types::sign::threshold_sig::public_key::bls12_381::PublicKeyBytes;
use ic_types::{
    crypto::{AlgorithmId, CryptoError, CryptoResult},
    NodeIndex, NumberOfNodes,
};

/// Domain separator for Hash-to-G1 to be used for signature generation as
/// as specified in the Basic ciphersuite in https://tools.ietf.org/html/draft-irtf-cfrg-bls-signature-04#section-4.2.1
const DOMAIN_HASH_MSG_TO_G1_BLS12381_SIG: &[u8; 43] =
    b"BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_NUL_";

/// Hashes `msg` to a point in `G1`.
fn hash_message_to_g1(msg: &[u8]) -> G1Projective {
    G1Projective::hash(&DOMAIN_HASH_MSG_TO_G1_BLS12381_SIG[..], msg)
}

#[cfg(test)]
pub mod tests;

/// Computes the public equivalent of a secret key.
pub fn public_key_from_secret_key(secret_key: &SecretKey) -> PublicKey {
    PublicKey(G2Affine::generator() * secret_key)
}

/// Yields the polynomial-evaluation point `x` given the `index` of the
/// corresponding share.
///
/// The polynomial `f(x)` is computed at a value `x` for every share of a
/// threshold key. Shares are ordered and numbered `0...N`.
pub(crate) fn x_for_index(index: NodeIndex) -> Scalar {
    // It is important that this is never zero and that values are unique.
    Scalar::from_u64(u64::from(index)) + Scalar::one()
}

/// Generates keys for a (t,n)-threshold signature scheme.
///
/// At least `t=threshold` contributions out of `n` are required to combine
/// the individual signatures.
///
/// The API supports dealing the n shares to a subset of N>=n actors by using a
/// vector to indicate which of the N actors should receive shares.
///
/// The `n` individual secret keys consist of the evaluation of a
/// random polynomial of length `threshold` (degree `threshold-1`) at a fixed
/// set of points. The public key consists of the group elements `C_i=[c_i]*G`
/// resulting from the scalar multiplication of the base point `G` with the
/// coefficients `c_i` of the polynomial. We denote them as
/// "public_coefficients".
///
/// # Arguments
/// * `seed` - randomness used to seed the PRNG for generating the polynomial.
///   It must be treated as a secret.
/// * `threshold` - the minimum number of individual signatures that can be
///   combined into a valid threshold signature.
/// * `receivers` - the number of receivers
///
/// # Errors
/// * `InvalidArgumentError` if
///   - The number of receivers is too large to be stored as type
///     `NumberOfNodes`.
///   - The number of eligible receivers is below the threshold; under these
///     circumstances the receivers could never generate a valid threshold key.
pub(crate) fn generate_threshold_key(
    seed: Seed,
    threshold: NumberOfNodes,
    receivers: NumberOfNodes,
) -> Result<(PublicCoefficients, Vec<SecretKey>), InvalidArgumentError> {
    verify_keygen_args(threshold, receivers)?;
    let mut rng = seed.into_rng();
    let polynomial = Polynomial::random(threshold.get() as usize, &mut rng);
    Ok(keygen_from_polynomial(polynomial, receivers))
}

/// Generates keys for a (t,n)-threshold signature scheme, resharing an existing
/// secret key.
///
/// This method is identical to `generate_threshold_key(..)` except that the threshold secret
/// key is specified (i.e. the constant term of the randomly-generated
/// polynomial is set to `secret`).
///
/// # Arguments
/// * `seed` - randomness used to seed the PRNG for generating the polynomial.
///   It must be treated as a secret.
/// * `threshold` - the minimum number of individual signatures that can be
///   combined into a valid threshold signature. (aka, t)
/// * `receivers` - the number of receivers (aka, n)
/// * `secret` - an existing secret key, which is to be shared.
///
/// # Errors
/// This returns an error if:
/// * The number of share indices is too large to be stored as type
///   NumberOfNodes.
/// * The number of eligible receivers is below the threshold; under these
///   circumstances the receivers could never generate a valid threshold key.
/// * The `threshold` is `0`.
pub(crate) fn threshold_share_secret_key(
    seed: Seed,
    threshold: NumberOfNodes,
    receivers: NumberOfNodes,
    secret: &SecretKey,
) -> Result<(PublicCoefficients, Vec<SecretKey>), InvalidArgumentError> {
    verify_keygen_args(threshold, receivers)?;
    // If a secret is provided we have one additional constraint:
    if threshold == NumberOfNodes::from(0) {
        return Err(InvalidArgumentError {
            message: format!(
                "Threshold cannot be zero if the zero coefficient is provided: (threshold={})",
                threshold.get(),
            ),
        });
    }

    let mut rng = seed.into_rng();
    let polynomial = {
        let mut polynomial = Polynomial::random(threshold.get() as usize, &mut rng);
        polynomial.coefficients[0] = secret.clone();
        polynomial
    };
    Ok(keygen_from_polynomial(polynomial, receivers))
}

/// Verifies that the keygen args are satisfiable.
///
/// # Arguments
/// * `threshold` - the minimum number of individual signatures that can be
///   combined into a valid threshold signature.
/// * `receivers` - the total number of shares that are created
/// # Errors
/// This returns an error if:
/// * The number of eligible receivers is below the threshold; under these
///   circumstances the receivers could never generate a valid threshold key.
fn verify_keygen_args(
    threshold: NumberOfNodes,
    receivers: NumberOfNodes,
) -> Result<(), InvalidArgumentError> {
    if threshold > receivers {
        return Err(InvalidArgumentError {
            message: format!(
                "Threshold too high: (threshold={} !<= {}=num_shares)",
                threshold, receivers,
            ),
        });
    }
    Ok(())
}

/// Generates keys from a polynomial
fn keygen_from_polynomial(
    polynomial: Polynomial,
    receivers: NumberOfNodes,
) -> (PublicCoefficients, Vec<SecretKey>) {
    let public_coefficients = PublicCoefficients::from(&polynomial);
    let shares = (0..receivers.get())
        .map(|idx| polynomial.evaluate_at(&x_for_index(idx)))
        .collect();
    (public_coefficients, shares)
}

/// Computes the public key of the `index`'th share from the given
/// public coefficients of the polynomial.
pub(crate) fn individual_public_key(
    public_coefficients: &PublicCoefficients,
    index: NodeIndex,
) -> PublicKey {
    PublicKey(public_coefficients.evaluate_at(&x_for_index(index)))
}

/// Computes the public key used to verify combined signatures.
///
/// When signatures are combined, they yield the same result as a single
/// signature with the secret key `polynomial.evaluated_at(0)`, i.e. the
/// constant term of the polynomial.  The corresponding public key is the first
/// element of the public coefficients.
///
/// Note: polynomial.evaluated_at(0) != polynomial.evaluated_at(x_for_index(0)).
pub fn combined_public_key(public_coefficients: &PublicCoefficients) -> PublicKey {
    PublicKey::from(public_coefficients)
}

/// Signs a message with the given secret key.
///
/// Note:  As the whole message needs to be provided, this is unsuitable for
/// signing large chunks of data or streaming data.  For large chunks of data
/// it is better to hash the data separately and provide the digest to
///   sign_hash(digest: [u8: 32], secret_key: &SecretKey) // unimplemented.
pub(crate) fn sign_message(message: &[u8], secret_key: &SecretKey) -> Signature {
    hash_message_to_g1(message) * secret_key
}

/// Combines signature shares (i.e. evaluates the signature at `x=0`).
///
/// Note: The threshold signatories are indexed from `0` to `num_signatories-1`.
/// The index of each signatory defines the x-value at which the the signature
/// is computed, so is needed along with the signature for the signature to be
/// useful.  Signatures are given in the same order as the signatories.  Missing
/// signatures are represented by `None`.
///
/// # Errors
/// * `CryptoError::InvalidArgument` if the given signature shares are lower
///   than the given threshold.
pub(crate) fn combine_signatures(
    signatures: &[Option<Signature>],
    threshold: NumberOfNodes,
) -> CryptoResult<Signature> {
    if threshold.get() as usize > signatures.iter().filter(|s| s.is_some()).count() {
        return Err(CryptoError::InvalidArgument {
            message: format!(
                "Threshold too high: (threshold={} !<= {}=num_shares)",
                threshold.get(),
                signatures.iter().filter(|s| s.is_some()).count()
            ),
        });
    }
    if signatures.is_empty() {
        return Ok(Signature::identity());
    }
    let signatures: Vec<(Scalar, Signature)> = signatures
        .iter()
        .cloned()
        .zip(0_u32..)
        .filter_map(|(signature, index)| signature.map(|signature| (x_for_index(index), signature)))
        .collect();
    Ok(PublicCoefficients::interpolate_g1(&signatures).expect("Duplicate indices"))
}

/// Verifies an individual signature against the provided public key.
///
/// # Returns
/// * OK, if `signature` is a valid BLS signature on `message`
/// * Err, otherwise
pub(crate) fn verify_individual_sig(
    message: &[u8],
    signature: &IndividualSignature,
    public_key: &PublicKey,
) -> CryptoResult<()> {
    verify(message, signature, public_key).map_err(|_| CryptoError::SignatureVerification {
        algorithm: AlgorithmId::ThresBls12_381,
        public_key_bytes: PublicKeyBytes::from(public_key).0.to_vec(),
        sig_bytes: IndividualSignatureBytes::from(signature).0.to_vec(),
        internal_error: "Invalid individual threshold signature".to_string(),
    })
}

/// Verifies a combined signature against the provided public key.
///
/// # Returns
/// * OK, if `signature` is a valid BLS signature on `message`
/// * Err, otherwise
pub(crate) fn verify_combined_sig(
    message: &[u8],
    signature: &CombinedSignature,
    public_key: &PublicKey,
) -> CryptoResult<()> {
    verify(message, signature, public_key).map_err(|_| CryptoError::SignatureVerification {
        algorithm: AlgorithmId::ThresBls12_381,
        public_key_bytes: PublicKeyBytes::from(public_key).0.to_vec(),
        sig_bytes: CombinedSignatureBytes::from(signature).0.to_vec(),
        internal_error: "Invalid combined threshold signature".to_string(),
    })
}

/// Verifies an individual or combined signature against the provided public
/// key.
// TODO(DFN-1408): Optimize signature verification by combining the miller
// loops inside the pairings, thus performing only a single final
// exponentiation.
fn verify(message: &[u8], signature: &Signature, public_key: &PublicKey) -> Result<(), ()> {
    let point = hash_message_to_g1(message).to_affine();
    let pk = public_key.0.to_affine();

    if verify_bls_signature(&signature.into(), &pk, &point) {
        Ok(())
    } else {
        Err(())
    }
}
