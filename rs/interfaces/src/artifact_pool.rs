//! The artifact pool public interface.
use derive_more::From;
use ic_types::{replica_version::ReplicaVersion, CountBytes, NodeId, Time};
use serde::{Deserialize, Serialize};

/// Contains different errors that can happen on artifact acceptance check.
/// In our P2P protocol none of the errors from 'ArtifactPoolError' are
/// handled by the caller. So the enum is used only for tracking different
/// rejection reasons.
#[derive(Debug, From)]
pub enum ArtifactPoolError {
    /// Error if not enough quota for a peer in the unvalidated pool for an
    /// artifact.
    InsufficientQuotaError,
    /// Message has expired.
    MessageExpired,
    /// Message expiry is too far in the future.
    MessageExpiryTooLong,
    /// Error when artifact version is not accepted.
    ArtifactReplicaVersionError(ReplicaVersionMismatch),
}

/// Describe expected version and artifact version when there is a mismatch.
#[derive(Debug)]
pub struct ReplicaVersionMismatch {
    pub expected: ReplicaVersion,
    pub artifact: ReplicaVersion,
}

/// Validated artifact
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedArtifact<T> {
    pub msg: T,
    pub timestamp: Time,
}

impl<T> ValidatedArtifact<T> {
    pub fn map<U, F>(self, f: F) -> ValidatedArtifact<U>
    where
        F: FnOnce(T) -> U,
    {
        ValidatedArtifact {
            msg: f(self.msg),
            timestamp: self.timestamp,
        }
    }
}

/// Unvalidated artifact
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnvalidatedArtifact<T> {
    pub message: T,
    pub peer_id: NodeId,
    pub timestamp: Time,
}

// Traits for accessing data for (un)validated artifacts follow.

impl<T: CountBytes> CountBytes for ValidatedArtifact<T> {
    fn count_bytes(&self) -> usize {
        self.msg.count_bytes() + self.timestamp.count_bytes()
    }
}

impl<T> AsRef<T> for ValidatedArtifact<T> {
    fn as_ref(&self) -> &T {
        &self.msg
    }
}

impl<T> AsRef<T> for UnvalidatedArtifact<T> {
    fn as_ref(&self) -> &T {
        &self.message
    }
}

/// A trait similar to Into, but without its restrictions.
pub trait IntoInner<T>: AsRef<T> {
    fn into_inner(self) -> T;
}

impl<T> IntoInner<T> for ValidatedArtifact<T> {
    fn into_inner(self) -> T {
        self.msg
    }
}

impl<T> IntoInner<T> for UnvalidatedArtifact<T> {
    fn into_inner(self) -> T {
        self.message
    }
}

/// A trait to get timestamp.
pub trait HasTimestamp {
    fn timestamp(&self) -> Time;
}

impl<T> HasTimestamp for ValidatedArtifact<T> {
    fn timestamp(&self) -> Time {
        self.timestamp
    }
}

impl<T> HasTimestamp for UnvalidatedArtifact<T> {
    fn timestamp(&self) -> Time {
        self.timestamp
    }
}
