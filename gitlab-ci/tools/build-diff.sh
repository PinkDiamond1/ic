#!/usr/bin/env bash
#
# Script for comparing artifacts in AWS S3 that are produced from our CI
#
# We build and push artifacts to S3:
# * [bazel] //publish/canister      -> ic/<sha256>/canisters
# * [bazel] //publish/binaries      -> ic/<sha256>/binaries
# * [bazel] //ic-os/guestos/prod:*  -> ic/<sha256>/guest-os/update-img
# We build the same set of artifacts:
# * build-ic                        -> ic/<sha256>/build-ic/canisters
#                                   -> ic/<sha256>/build-ic/binaries
#                                   -> ic/<sha256>/build-ic/icos/guest-os/update-img
#
# This script compares SHA256SUMS file [diff] and also the actual artifacts [diffoscope]

usage() {
    echo -e "Usage: $0 <path-0> <path-1> [<git-revision>]"
    echo -e ""
    echo -e "HEAD revision:"
    echo -e ""
    echo -e "\t$0 build-ic/canisters canisters [diff]"
    echo -e "\t$0 build-ic/release release [diff]"
    echo -e "\t$0 build-ic/release/replica.gz release/replica.gz [diffoscope]"
    echo -e "\t$0 build-ic/guest-os/update-img guest-os/update-img [diff/diffoscope]"
    echo -e ""
    echo -e "Specific revision:"
    echo -e ""
    echo -e "\t$0 /<sha256>/build-ic/release release [diff]"
    echo -e "\t$0 /<sha256>/build-ic/release /<sha256'>/release [diff]"
    echo -e "\t$0 /<sha256>/build-ic/guest-os/update-img /<sha256>/guest-os/update-img [diff/diffoscope]"
    echo -e ""
    echo -e "Note: <sha256>/<sha256'> is git revision and must be full, 40 char string."
}

diffoscope_check() {
    if ! which diffoscope; then
        if grep -q Ubuntu /etc/os-release; then
            sudo apt-get update && sudo apt-get --no-install-recommends --yes install \
                "linux-image-$(uname -r)" diffoscope \
                python3-tlsh libguestfs-tools python3-guestfs squashfs-tools
        else
            echo "No diffoscope found!" && exit 1
        fi
    fi
}

alert() {
    # no alert if this is not a scheduled CI job!
    if [ ${CI_PIPELINE_SOURCE:-} != "schedule" ]; then
        exit 1
    fi

    MESSAGE="Release Build Reproducibility Failure in <$CI_JOB_URL|$CI_JOB_NAME>! "
    MESSAGE+="Follow <http://go/reproducible-builds-incident-runbook|this run-book>! "
    MESSAGE+="<!subteam^S022UEH2AKE>"
    # https://stackoverflow.com/questions/54284389/mention-users-group-via-slack-api

    ./gitlab-ci/src/notify_slack/notify_slack.py \
        "$MESSAGE" --channel "#eng-idx"

    exit 1
}

if [ $# -lt 2 ]; then
    usage
    exit 1
fi

set -exuo pipefail

PATH0=$1
PATH1=$2
VERSION=${3:-$(git rev-parse HEAD)}

# relative path doesn't include sha256
if [[ ${PATH0::1} != '/' ]]; then
    PATH0="${VERSION}/${PATH0}"
else
    PATH0="${PATH0:1}"
fi

# relative path doesn't include sha256
if [[ ${PATH1::1} != '/' ]]; then
    PATH1="${VERSION}/${PATH1}"
else
    PATH1="${PATH1:1}"
fi

if [[ $PATH0 == *".gz" && $PATH1 == *".gz" ]]; then
    mkdir -p "$(dirname /tmp/$PATH0)"
    mkdir -p "$(dirname /tmp/$PATH1)"

    curl -sfS --retry 5 --retry-delay 10 \
        "https://download.dfinity.systems/ic/$PATH0" \
        -o "/tmp/$PATH0"
    curl -sfS --retry 5 --retry-delay 10 \
        "https://download.dfinity.systems/ic/$PATH1" \
        -o "/tmp/$PATH1"

    diffoscope_check
    diffoscope --html-dir "diffoscope-${VERSION}" "/tmp/$PATH0" "/tmp/$PATH1"

    exit 0
fi

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT
SHA256SUMS0="$TMPDIR/SHA256SUMS0"
SHA256SUMS1="$TMPDIR/SHA256SUMS01"
rm -f $SHA256SUMS0 $SHA256SUMS1

curl -sfS --retry 5 --retry-delay 10 \
    "https://download.dfinity.systems/ic/$PATH0/SHA256SUMS" \
    -o $SHA256SUMS0
curl -sfS --retry 5 --retry-delay 10 \
    "https://download.dfinity.systems/ic/$PATH1/SHA256SUMS" \
    -o $SHA256SUMS1

echo "$PATH0/SHA256SUMS:"
cat $SHA256SUMS0
echo "$PATH1/SHA256SUMS:"
cat $SHA256SUMS1

echo "Full diff before dropping artifacts allowed non-determinism"
diff -u "$SHA256SUMS0" "$SHA256SUMS1" || true

# TODO(IDX-2542)
sed -i -e '/panics.wasm/d' -e '/ic-rosetta-api/d' -e '/system-tests/d' -e'/prod-test-driver/d' -e'/sns-test-dapp-canister/d' $SHA256SUMS0 $SHA256SUMS1

# build-ic produces guest and update img so we need to filter guest img out
sed -i -e '/disk-img/d' $SHA256SUMS0 $SHA256SUMS1

if ! diff -u $SHA256SUMS0 $SHA256SUMS1; then
    set +x
    echo -e "\nThis script compares artifacts built from separate CI jobs"
    set -x

    if grep -q "update-img" $SHA256SUMS0; then
        echo "Running diffoscope for update-img"
        diffoscope_check

        ARTIFACT="update-img.tar.gz"
        if grep -q "host-update-img" $SHA256SUMS0; then
            ARTIFACT="host-update-img.tar.gz"
        fi

        mkdir -p "$PATH0" "$PATH1" artifacts
        curl -sfS --retry 5 --retry-delay 10 \
            "https://download.dfinity.systems/ic/$PATH0/$ARTIFACT" \
            -o "$PATH0/$ARTIFACT"
        curl -sfS --retry 5 --retry-delay 10 \
            "https://download.dfinity.systems/ic/$PATH1/$ARTIFACT" \
            -o "$PATH1/$ARTIFACT"

        pushd "$PATH0"
        tar -xzf "$ARTIFACT"
        popd
        pushd "$PATH1"
        tar -xzf "$ARTIFACT"
        popd

        # we give diffoscope 20min to find the diff
        TRIGGER_ALERT=false
        timeout 20m sudo diffoscope \
            "$PATH0/boot.img" \
            "$PATH1/boot.img" \
            --html artifacts/output-boot.html --text - || TRIGGER_ALERT=true
        timeout 20m sudo diffoscope \
            "$PATH0/root.img" \
            "$PATH1/root.img" \
            --html artifacts/output-root.html --text - || TRIGGER_ALERT=true
        if [ "$TRIGGER_ALERT" == true ]; then
            alert
        fi
    else
        set +x
        echo -e "Investigate with diffoscope [\xF0\x9F\x99\x8F]:"
        echo "  BIN=ic-admin.gz # (specify the right artifact)"
        echo "  $0 /${PATH0}/\$BIN /${PATH1}/\$BIN $VERSION"
        set -x

        alert
    fi

else
    echo "Build Determinism Check Successful"
fi
