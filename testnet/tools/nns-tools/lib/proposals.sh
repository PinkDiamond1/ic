#!/bin/bash

#### Proposal generators

generate_sale_canister_upgrade_proposal_text() {
    local LAST_COMMIT=$1
    local NEXT_COMMIT=$2
    local CANISTER_ID=$3
    local OUTPUT_FILE=${4:-}

    WASM_GZ=$(get_sns_canister_wasm_gz_for_type "swap" "$NEXT_COMMIT")
    WASM_SHA=$(sha_256 "$WASM_GZ")
    CAPITALIZED_CANISTER_NAME="Swap"
    LAST_WASM_HASH=$(canister_hash ic $CANISTER_ID)

    IC_REPO=$(repo_root)

    CANISTER_CODE_LOCATION=$(get_sns_canister_code_location swap)
    ESCAPED_IC_REPO=$(printf '%s\n' "$IC_REPO" | sed -e 's/[]\/$*.^[]/\\&/g')
    RELATIVE_CODE_LOCATION=$(echo "$CANISTER_CODE_LOCATION" | sed "s/$ESCAPED_IC_REPO/./g")

    OUTPUT=$(
        cat <<EOF
## Proposal to Upgrade the Sale Canister for TODO
### Proposer: DFINITY Foundation
### Git Hash: $NEXT_COMMIT
### New Wasm Hash: $WASM_SHA
### Target canister: $CANISTER_ID
---
## Features
TODO ADD FEATURE NOTES
## Release Notes
\`\`\`
\$ git log --format="%C(auto) %h %s" $LAST_COMMIT..$NEXT_COMMIT --  $RELATIVE_CODE_LOCATION
$(git log --format="%C(auto) %h %s" "$LAST_COMMIT".."$NEXT_COMMIT" -- "$CANISTER_CODE_LOCATION")
\`\`\`
## Wasm Verification
Verify that the hash of the gzipped WASM matches the proposed hash.
\`\`\`
git fetch
git checkout $NEXT_COMMIT
./gitlab-ci/container/build-ic.sh -c
sha256sum ./artifacts/canisters/$(_canister_download_name_for_sns_canister_type swap).wasm.gz
\`\`\`
## Current Version
- Current Git Hash: $LAST_COMMIT
- Current Wasm Hash: $LAST_WASM_HASH
EOF
    )

    if [ -z "$OUTPUT_FILE" ]; then
        echo "$OUTPUT"
    else
        echo "$OUTPUT" >"$OUTPUT_FILE"
        echo "File created at $OUTPUT_FILE"
    fi

}

generate_nns_proposal_text() {

    local LAST_COMMIT=$1
    local NEXT_COMMIT=$2
    local CANISTER_NAME=$3
    local OUTPUT_FILE=${4:-}

    WASM_GZ=$(get_nns_canister_wasm_gz_for_type "$CANISTER_NAME" "$NEXT_COMMIT")
    WASM_SHA=$(sha_256 "$WASM_GZ")
    CAPITALIZED_CANISTER_NAME="$(tr '[:lower:]' '[:upper:]' <<<${CANISTER_NAME:0:1})${CANISTER_NAME:1}"
    LAST_WASM_HASH=$(nns_canister_hash ic "$CANISTER_NAME")

    IC_REPO=$(repo_root)

    CANISTER_CODE_LOCATION=$(get_nns_canister_code_location "$CANISTER_NAME")
    ESCAPED_IC_REPO=$(printf '%s\n' "$IC_REPO" | sed -e 's/[]\/$*.^[]/\\&/g')
    RELATIVE_CODE_LOCATION="$(echo "$CANISTER_CODE_LOCATION" | sed "s/$ESCAPED_IC_REPO/./g")"

    OUTPUT=$(
        cat <<EOF
## Proposal to Upgrade the $CAPITALIZED_CANISTER_NAME Canister
### Proposer: DFINITY Foundation
### Git Hash: $NEXT_COMMIT
### New Wasm Hash: $WASM_SHA
### Target canister: $(nns_canister_id "$CANISTER_NAME")
---
## Features
TODO ADD FEATURE NOTES
## Release Notes
\`\`\`
\$ git log --format="%C(auto) %h %s" $LAST_COMMIT..$NEXT_COMMIT --  $RELATIVE_CODE_LOCATION
$(git log --format="%C(auto) %h %s" "$LAST_COMMIT".."$NEXT_COMMIT" -- $CANISTER_CODE_LOCATION)
\`\`\`
## Wasm Verification
Verify that the hash of the gzipped WASM matches the proposed hash.
\`\`\`
git fetch
git checkout $NEXT_COMMIT
./gitlab-ci/container/build-ic.sh -c
sha256sum ./artifacts/canisters/$(_canister_download_name_for_nns_canister_type "$CANISTER_NAME").wasm.gz
\`\`\`
## Current Version
- Current Git Hash: $LAST_COMMIT
- Current Wasm Hash: $LAST_WASM_HASH
EOF
    )

    if [ -z "$OUTPUT_FILE" ]; then
        echo "$OUTPUT"
    else
        echo "$OUTPUT" >"$OUTPUT_FILE"
        echo "File created at $OUTPUT_FILE"
    fi

}

generate_sns_bless_wasm_proposal_text() {

    local LAST_COMMIT=$1
    local NEXT_COMMIT=$2
    local CANISTER_TYPE=$3
    local OUTPUT_FILE=${4:-}

    WASM_GZ=$(get_sns_canister_wasm_gz_for_type "$CANISTER_TYPE" "$NEXT_COMMIT")
    WASM_SHA=$(sha_256 "$WASM_GZ")
    CAPITALIZED_CANISTER_TYPE="$(tr '[:lower:]' '[:upper:]' <<<${CANISTER_TYPE:0:1})${CANISTER_TYPE:1}"

    IC_REPO=$(repo_root)

    CANISTER_CODE_LOCATION=$(get_sns_canister_code_location "$CANISTER_TYPE")
    ESCAPED_IC_REPO=$(printf '%s\n' "$IC_REPO" | sed -e 's/[]\/$*.^[]/\\&/g')
    RELATIVE_CODE_LOCATION="$(echo "$CANISTER_CODE_LOCATION" | sed "s/$ESCAPED_IC_REPO/./g")"

    OUTPUT=$(
        cat <<EOF
## Proposal to Publish the SNS $CAPITALIZED_CANISTER_TYPE Canister WASM to SNS-W
### Proposer: DFINITY Foundation
### Canister Type: $CANISTER_TYPE
### Git Hash: $NEXT_COMMIT
### New Wasm Hash: $WASM_SHA
---
## Features
TODO ADD FEATURE NOTES
## Release Notes
\`\`\`
\$ git log --format="%C(auto) %h %s" $LAST_COMMIT..$NEXT_COMMIT --  $RELATIVE_CODE_LOCATION
$(git log --format="%C(auto) %h %s" "$LAST_COMMIT".."$NEXT_COMMIT" -- $CANISTER_CODE_LOCATION)
\`\`\`
## Wasm Verification
Verify that the hash of the gzipped WASM matches the proposed hash.
\`\`\`
git fetch
git checkout $NEXT_COMMIT
./gitlab-ci/container/build-ic.sh -c
sha256sum ./artifacts/canisters/$(_canister_download_name_for_sns_canister_type "$CANISTER_TYPE").wasm.gz
\`\`\`
EOF
    )

    if [ -z "$OUTPUT_FILE" ]; then
        echo "$OUTPUT"
    else
        echo "$OUTPUT" >"$OUTPUT_FILE"
        echo "File created at $OUTPUT_FILE"
    fi

}

generate_insert_custom_upgrade_paths_proposal_text() {
    local SNS_GOVERNANCE_CANISTER_ID=$1
    shift
    VERSIONS=()
    for ((c = 1; c <= $#; c++)); do
        VERSIONS+=("${!c}")
    done

    DESCRIPTION=$([ "$SNS_GOVERNANCE_CANISTER_ID" == "" ] \
        && echo "All SNS upgrade paths (without their own overrides) will be affected by this proposal." \
        || echo "This will only affect the SNS that has the following governance Canister ID: $SNS_GOVERNANCE_CANISTER_ID.")
    DISPLAY_GOVERNANCE_ID=$([ "$SNS_GOVERNANCE_CANISTER_ID" == "" ] && echo "All" || echo "$SNS_GOVERNANCE_CANISTER_ID")
    GOVERNANCE_CANDID_ARG=$([ "$SNS_GOVERNANCE_CANISTER_ID" == "" ] \
        && echo "" \
        || echo "sns_governance_canister_id = opt principal \"$SNS_GOVERNANCE_CANISTER_ID\";")

    LAST_VERSION=""
    OUTPUT=$(
        cat <<EOF
## Proposal to TODO
### Proposer: DFINITY Foundation
### Target SNS Governance Canister: $DISPLAY_GOVERNANCE_ID
---
This proposal will change the upgrade path to use different WASMs, but WASMs that are already available on SNS-W.

$DESCRIPTION

## Rationale

TODO

## Useful background

To see what the upgrade path currently is, run:
\`\`\`
dfx canister --network ic call --candid ic/rs/nns/sns-wasm/canister/sns-wasm.did \\
    qaa6y-5yaaa-aaaaa-aaafa-cai list_upgrade_steps \\
    '(record {limit = 0: nat32; $GOVERNANCE_CANDID_ARG})'
\`\`\`
$(
            [ "$SNS_GOVERNANCE_CANISTER_ID" != "" ] && cat <<EO2

To see the current version the SNS reports to be running:
\`\`\`
dfx canister --network ic \\
        call --candid ic/rs/sns/governance/canister/governance.did \\
        "$SNS_GOVERNANCE_CANISTER_ID" get_running_sns_version "(record{})"
\`\`\`

EO2
        )
## Upgrade Path Changes

$(for VERSION in "${VERSIONS[@]}"; do
            if [ "$LAST_VERSION" != "" ]; then
                # Combine the upgrades to emulate the way this will work
                VERSION=$(echo "[$LAST_VERSION, $VERSION]" | jq -cS '.[0] * .[1]')
            else

                VERSION=$(echo $VERSION | jq -cS .)
            fi
            echo $VERSION | jq .
            echo
            LAST_VERSION=$VERSION
        done)

EOF
    )

    echo "$OUTPUT"
}

#### Proposal value extractors (based on common format of proposal elements)

# Extracts "LAST_COMMIT" from string like "git log $LAST_COMMIT..$NEXT_COMMIT" where commits are git commit ids
# Usage extract_previous_version <PROPOSAL_FILE>
extract_previous_version() {
    local FILE=$1
    cat $FILE | grep "git log" | sed 's/.*\([0-9a-f]\{40\}\)\.\.[0-9a-f]\{40\}.*/\1/'
}

# Extracts a proposal header field value if the field title is given.
# Example:
#   For file with line like: "### Some Field: foo"
#   the value of foo can be extracted with "proposal_header_field_value <FILE> 'Some Field:'"
# Usage: proposal_header_field_value <FILE> <FIELD_NAME>
proposal_header_field_value() {
    local FILE=$1
    local FIELD=$2
    cat $FILE | grep "### $FIELD" | sed "s/.*$FIELD[[:space:]]*//"
}

#### Proposal text validators

validate_no_todos() {
    local PROPOSAL_FILE=$1

    if grep -q -i TODO "$PROPOSAL_FILE"; then
        echo >&2 "Cannot submit proposal with 'TODO' items in text"
        exit 1
    fi
}

validate_nns_canister_id() {
    local CANISTER_NAME=$1
    local EXPECTED_CANISTER_ID=$2

    local CALCULATED_CANISTER_ID=$(nns_canister_id "$CANISTER_NAME")

    if [ "$EXPECTED_CANISTER_ID" != "$CALCULATED_CANISTER_ID" ]; then
        echo "Target canister does not match expected value for named canister in proposal"
        return 1
    fi
}

validate_nns_version_wasm_sha() {
    local CANISTER_TYPE=$1 # Same as CANISTER_NAME for nns canisters
    local VERSION=$2
    local EXPECTED_SHA=$3

    _base_validate_version_wasm_sha \
        $(get_nns_canister_wasm_gz_for_type "$CANISTER_TYPE" "$VERSION") \
        "$EXPECTED_SHA"
}

validate_sns_version_wasm_sha() {
    local CANISTER_TYPE=$1
    local VERSION=$2
    local EXPECTED_SHA=$3

    _base_validate_version_wasm_sha \
        $(get_sns_canister_wasm_gz_for_type "$CANISTER_TYPE" "$VERSION") \
        "$EXPECTED_SHA"
}

_base_validate_version_wasm_sha() {
    local WASM_GZ=$1
    local EXPECTED_SHA=$2

    WASM_SHA=$(sha_256 "$WASM_GZ")

    if [ "$WASM_SHA" != "$EXPECTED_SHA" ]; then
        echo "SHA256 hash for WASM at proposed version does not match hash stated in proposal"
        exit 1
    fi
}

#### User interaction helper for proposals

confirm_submit_proposal_command() {
    echo "Would you like to run the following command?"
    echo
    print_blue "$@" | sed 's/pin=[0-9]*/pin=\*\*\*\*\*\*/' | fold -w 120 -s | sed -e "s|^|     |g"
    echo
    confirm
}
