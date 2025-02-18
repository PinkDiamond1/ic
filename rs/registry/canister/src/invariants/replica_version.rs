use std::collections::BTreeSet;

use crate::invariants::common::{
    get_subnet_ids_from_snapshot, get_value_from_snapshot, InvariantCheckError, RegistrySnapshot,
};

use url::Url;

use ic_base_types::SubnetId;
use ic_nns_common::registry::decode_or_panic;
use ic_protobuf::registry::{
    replica_version::v1::{BlessedReplicaVersions, ReplicaVersionRecord},
    subnet::v1::SubnetRecord,
    unassigned_nodes_config::v1::UnassignedNodesConfigRecord,
};
use ic_registry_keys::{
    make_blessed_replica_versions_key, make_replica_version_key, make_subnet_record_key,
    make_unassigned_nodes_config_record_key,
};

/// A predicate on the replica version records contained in a registry
/// snapshot.
///
/// For each replica version that is either referred to in an SubnetRecord
/// of a subnet that is listed in the subnet list or that is contained
/// the BlessedReplicaVersions-List, the following is checked:
///
/// * The corresponding ReplicaVersionRecord exists.
/// * Each URL is well-formed.
/// * Release package hash is a well-formed hex-encoded SHA256 value.
pub(crate) fn check_replica_version_invariants(
    snapshot: &RegistrySnapshot,
) -> Result<(), InvariantCheckError> {
    let mut versions_in_use = get_all_replica_versions_of_subnets(snapshot);
    let unassigned_version_id = snapshot
        .get(make_unassigned_nodes_config_record_key().as_bytes())
        .map(|bytes| {
            let unassigned_nodes_config =
                decode_or_panic::<UnassignedNodesConfigRecord>(bytes.clone());
            unassigned_nodes_config.replica_version
        });
    if let Some(version) = unassigned_version_id {
        versions_in_use.insert(version);
    }

    let blessed_version_ids = snapshot
        .get(make_blessed_replica_versions_key().as_bytes())
        .map(|bytes| {
            let version_list = decode_or_panic::<BlessedReplicaVersions>(bytes.clone());
            version_list.blessed_version_ids
        })
        .unwrap_or_else(Vec::default);

    let num_blessed = blessed_version_ids.len();
    let blessed_set = BTreeSet::from_iter(blessed_version_ids);
    assert!(
        blessed_set.len() == num_blessed,
        "A version was blessed multiple times."
    );
    assert!(
        blessed_set.is_superset(&versions_in_use),
        "Using a version that isn't blessed. Blessed versions: {blessed_set:?}, in use: {versions_in_use:?}."
    );
    assert!(
        blessed_set.iter().all(|v| !v.trim().is_empty()),
        "Blessed an empty version ID."
    );

    // Check whether release package URLs (iso image) and corresponding hash is well-formed.
    // As file-based URLs are only used in test-deployments, we disallow file:/// URLs.
    for version in blessed_set {
        let r = get_replica_version_record(snapshot, version);
        assert_valid_urls_and_hash(
            &r.release_package_urls,
            &r.release_package_sha256_hex,
            false, // allow_file_url
        );
    }

    Ok(())
}

fn get_replica_version_record(
    snapshot: &RegistrySnapshot,
    version: String,
) -> ReplicaVersionRecord {
    get_value_from_snapshot(snapshot, make_replica_version_key(version.clone()))
        .unwrap_or_else(|| panic!("Could not find replica version: {version}"))
}

fn get_subnet_record(snapshot: &RegistrySnapshot, subnet_id: SubnetId) -> SubnetRecord {
    get_value_from_snapshot(snapshot, make_subnet_record_key(subnet_id))
        .unwrap_or_else(|| panic!("Could not get subnet record for subnet: {subnet_id}"))
}

/// Returns the list of replica versions where each version is referred to
/// by at least one subnet.
fn get_all_replica_versions_of_subnets(snapshot: &RegistrySnapshot) -> BTreeSet<String> {
    get_subnet_ids_from_snapshot(snapshot)
        .iter()
        .map(|subnet_id| get_subnet_record(snapshot, *subnet_id).replica_version_id)
        .collect()
}

fn assert_sha256(s: &str) {
    if s.bytes().any(|x| !x.is_ascii_hexdigit()) {
        panic!("Hash contains at least one invalid character: `{s}`");
    }
}

fn assert_valid_urls_and_hash(urls: &[String], hash: &str, allow_file_url: bool) {
    // Either both, the URL and the hash are set, or both are not set.
    if (urls.is_empty() as i32 ^ hash.is_empty() as i32) > 0 {
        panic!("Either both, an url and a hash must be set, or none.");
    }
    if urls.is_empty() {
        return;
    }

    assert_sha256(hash);

    urls.iter().for_each(|url|
        // File URLs are used in test deployments. We only disallow non-ASCII.
        if allow_file_url && url.starts_with("file://") {
            assert!(url.is_ascii(), "file-URL {url} contains non-ASCII characters.");
        }
        // if it's not a file URL, it should be a valid URL.
        else if let Err(e) = Url::parse(url) {
            panic!("Release package URL {url} is not valid: {e}");
        }
    );
}

#[cfg(test)]
mod tests {
    use crate::{
        common::test_helpers::invariant_compliant_registry, mutations::common::encode_or_panic,
    };

    use super::*;
    use canister_test::PrincipalId;
    use ic_registry_transport::{insert, upsert};
    use ic_types::ReplicaVersion;

    const MOCK_HASH: &str = "C0FFEE";
    const MOCK_URL: &str = "http://release_package.tar.gz";

    fn check_bless_version(versions: Vec<String>) {
        let registry = invariant_compliant_registry();

        let key = make_blessed_replica_versions_key();
        let value = encode_or_panic(&BlessedReplicaVersions {
            blessed_version_ids: versions,
        });

        let mutation = vec![insert(key.as_bytes(), value)];
        registry.check_global_state_invariants(&mutation);
    }

    #[test]
    #[should_panic(expected = "Blessed an empty version ID.")]
    fn panic_when_blessing_empty_version() {
        check_bless_version(vec![ReplicaVersion::default().into(), "".into()]);
    }

    #[test]
    #[should_panic(expected = "Blessed an empty version ID.")]
    fn panic_when_blessing_whitespace_version() {
        check_bless_version(vec![ReplicaVersion::default().into(), "  ".into()]);
    }

    #[test]
    #[should_panic(expected = "A version was blessed multiple times.")]
    fn panic_when_blessing_same_version_twice() {
        check_bless_version(vec!["version_a".into(), "version_a".into()]);
    }

    #[test]
    #[should_panic(expected = "Using a version that isn't blessed.")]
    fn panic_when_retiring_a_version_in_use() {
        check_bless_version(vec![]);
    }

    #[test]
    #[should_panic(expected = "Could not find replica version: unknown")]
    fn panic_when_blessing_unknown_version() {
        check_bless_version(vec![ReplicaVersion::default().into(), "unknown".into()]);
    }

    #[test]
    #[should_panic(expected = "Using a version that isn't blessed.")]
    fn panic_when_using_unelected_version() {
        let registry = invariant_compliant_registry();

        let list = registry.get_subnet_list_record();
        let nns_id = SubnetId::from(PrincipalId::try_from(list.subnets.get(0).unwrap()).unwrap());
        let mut subnet = registry.get_subnet_or_panic(nns_id);
        subnet.replica_version_id = "unelected".into();

        let new_subnet = upsert(
            make_subnet_record_key(nns_id).into_bytes(),
            encode_or_panic(&subnet),
        );
        registry.check_global_state_invariants(&[new_subnet]);
    }

    #[test]
    #[should_panic(expected = "Using a version that isn't blessed.")]
    fn panic_when_retiring_unassigned_nodes_version() {
        let mut registry = invariant_compliant_registry();

        let replica_version_id = "unassigned_version".to_string();
        let replica_version = ReplicaVersionRecord {
            release_package_sha256_hex: "".to_string(),
            release_package_urls: vec![],
            guest_launch_measurement_sha256_hex: None,
        };
        let blessed_replica_version = BlessedReplicaVersions {
            blessed_version_ids: vec![ReplicaVersion::default().into(), replica_version_id.clone()],
        };
        let unassigned_nodes_config = UnassignedNodesConfigRecord {
            ssh_readonly_access: vec![],
            replica_version: replica_version_id.clone(),
        };

        let init = vec![
            insert(
                make_replica_version_key(replica_version_id).as_bytes(),
                encode_or_panic(&replica_version),
            ),
            upsert(
                make_blessed_replica_versions_key().as_bytes(),
                encode_or_panic(&blessed_replica_version),
            ),
            insert(
                make_unassigned_nodes_config_record_key(),
                encode_or_panic(&unassigned_nodes_config),
            ),
        ];
        registry.maybe_apply_mutation_internal(init);

        let key = make_blessed_replica_versions_key();

        let value = encode_or_panic(&BlessedReplicaVersions {
            blessed_version_ids: vec![ReplicaVersion::default().into()],
        });

        let mutation = vec![insert(key.as_bytes(), value)];
        registry.check_global_state_invariants(&mutation);
    }

    #[test]
    #[should_panic(expected = "Using a version that isn't blessed.")]
    fn panic_when_using_unelected_unassigned_version() {
        let registry = invariant_compliant_registry();

        let key = make_unassigned_nodes_config_record_key();
        let value = encode_or_panic(&UnassignedNodesConfigRecord {
            ssh_readonly_access: vec![],
            replica_version: "unelected".into(),
        });

        let mutation = vec![insert(key.as_bytes(), value)];
        registry.check_global_state_invariants(&mutation);
    }

    fn check_replica_version(hash: &str, urls: Vec<String>) {
        let registry = invariant_compliant_registry();

        let key = make_replica_version_key(ReplicaVersion::default());
        let value = encode_or_panic(&ReplicaVersionRecord {
            release_package_sha256_hex: hash.into(),
            release_package_urls: urls,
            guest_launch_measurement_sha256_hex: None,
        });

        let mutation = vec![upsert(key.as_bytes(), value)];
        registry.check_global_state_invariants(&mutation);
    }

    #[test]
    #[should_panic(expected = "Either both, an url and a hash must be set, or none.")]
    fn panic_when_only_hash_is_set() {
        check_replica_version(MOCK_HASH, vec![]);
    }

    #[test]
    #[should_panic(expected = "Either both, an url and a hash must be set, or none.")]
    fn panic_when_only_url_is_set() {
        check_replica_version("", vec![MOCK_URL.into()]);
    }

    #[test]
    #[should_panic(expected = "Release package URL abcde is not valid")]
    fn panic_when_url_is_invalid() {
        check_replica_version(MOCK_HASH, vec!["abcde".into()]);
    }

    #[test]
    #[should_panic(expected = "Hash contains at least one invalid character")]
    fn panic_when_hash_is_invalid() {
        check_replica_version("XYZ", vec![MOCK_URL.into()]);
    }

    #[test]
    fn empty_hash_and_url() {
        check_replica_version("", vec![]);
    }

    #[test]
    fn set_hash_and_url() {
        check_replica_version(MOCK_HASH, vec![MOCK_URL.into()]);
    }
}
