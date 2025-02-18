"""
A macro to build multiple versions of the ICOS image (i.e., dev vs prod)
"""

load("//toolchains/sysimage:toolchain.bzl", "disk_image", "docker_tar", "ext4_image", "sha256sum", "tar_extract", "upgrade_image")
load("//gitlab-ci/src/artifacts:upload.bzl", "upload_artifacts")
load("//bazel:defs.bzl", "gzip_compress")
load("//bazel:output_files.bzl", "output_files")
load("@bazel_skylib//rules:copy_file.bzl", "copy_file")

def icos_build(name, upload_prefix, image_deps, mode = None, malicious = False, upgrades = True, vuln_scan = True, visibility = None):
    """
    Generic ICOS build tooling.

    Args:
      name: Name for the generated filegroup.
      upload_prefix: Prefix to be used as the target when uploading
      image_deps: Function to be used to generate image manifest
      mode: dev or prod. If not specified, will use the value of `name`
      malicious: if True, bundle the `malicious_replica`
      upgrades: if True, build upgrade images as well
      vuln_scan: if True, create targets for vulnerability scanning
      visibility: See Bazel documentation
    """

    if mode == None:
        mode = name

    image_deps = image_deps(mode, malicious)

    # -------------------- Version management --------------------

    # TODO(IDX-2538): re-enable this (or any other similar) solution when everything will be ready to have ic version that is not git revision.
    #summary_sha256sum(
    #    name = "version.txt",
    #    inputs = image_deps,
    #    suffix = "-dev" if mode == "dev" else "",
    #)

    copy_file(
        name = "copy_version_txt",
        src = "//bazel:version.txt",
        out = "version.txt",
        allow_symlink = True,
    )

    if upgrades:
        native.genrule(
            name = "test_version_txt",
            srcs = [":copy_version_txt"],
            outs = ["version-test.txt"],
            cmd = "sed -e 's/.*/&-test/' < $< > $@",
        )

    # -------------------- Build the docker image --------------------

    build_args = ["BUILD_TYPE=" + mode]

    # set root password only in dev mode
    if mode == "dev":
        build_args.extend(["ROOT_PASSWORD=root"])

    file_build_args = {image_deps["base_image"]: "BASE_IMAGE"}

    docker_tar(
        visibility = visibility,
        name = "rootfs-tree.tar",
        dep = [image_deps["docker_context"]],
        build_args = build_args,
        file_build_args = file_build_args,
        target_compatible_with = [
            "@platforms//os:linux",
        ],
    )

    tar_extract(
        visibility = visibility,
        name = "file_contexts",
        src = "rootfs-tree.tar",
        path = "etc/selinux/default/contexts/files/file_contexts",
        target_compatible_with = [
            "@platforms//os:linux",
        ],
    )

    # -------------------- Extract root partition --------------------

    ext4_image(
        name = "partition-root-unsigned.tar",
        src = _dict_value_search(image_deps["rootfs"], "/"),
        # Take the dependency list declared above, and add in the "version.txt"
        # at the correct place.
        extra_files = {
            k: v
            for k, v in (image_deps["rootfs"].items() + [(":version.txt", "/opt/ic/share/version.txt:0644")])
            # Skip over special entries
            if v != "/"
        },
        file_contexts = ":file_contexts",
        partition_size = image_deps["rootfs_size"],
        strip_paths = [
            "/run",
            "/boot",
        ],
        target_compatible_with = [
            "@platforms//os:linux",
        ],
    )

    if upgrades:
        ext4_image(
            name = "partition-root-test-unsigned.tar",
            src = _dict_value_search(image_deps["rootfs"], "/"),
            # Take the dependency list declared above, and add in the "version.txt"
            # at the correct place.
            extra_files = {
                k: v
                for k, v in (image_deps["rootfs"].items() + [(":version-test.txt", "/opt/ic/share/version.txt:0644")])
                # Skip over special entries
                if v != "/"
            },
            file_contexts = ":file_contexts",
            partition_size = image_deps["rootfs_size"],
            strip_paths = [
                "/run",
                "/boot",
            ],
            target_compatible_with = [
                "@platforms//os:linux",
            ],
        )

    # -------------------- Extract boot partition --------------------

    if "boot_args_template" not in image_deps:
        native.alias(name = "partition-root.tar", actual = ":partition-root-unsigned.tar", visibility = [Label("//visibility:private")])
        native.alias(name = "extra_boot_args", actual = image_deps["extra_boot_args"], visibility = [Label("//visibility:private")])

        if upgrades:
            native.alias(name = "partition-root-test.tar", actual = ":partition-root-test-unsigned.tar", visibility = [Label("//visibility:private")])
            native.alias(name = "extra_boot_test_args", actual = image_deps["extra_boot_args"], visibility = [Label("//visibility:private")])
    else:
        native.alias(name = "extra_boot_args_template", actual = image_deps["boot_args_template"], visibility = [Label("//visibility:private")])

        native.genrule(
            name = "partition-root-sign",
            srcs = ["partition-root-unsigned.tar"],
            outs = ["partition-root.tar", "partition-root-hash"],
            cmd = "$(location //toolchains/sysimage:verity_sign.py) -i $< -o $(location :partition-root.tar) -r $(location partition-root-hash)",
            executable = False,
            tools = ["//toolchains/sysimage:verity_sign.py"],
        )

        native.genrule(
            name = "extra_boot_args_root_hash",
            srcs = [
                ":extra_boot_args_template",
                ":partition-root-hash",
            ],
            outs = ["extra_boot_args"],
            cmd = "sed -e s/ROOT_HASH/$$(cat $(location :partition-root-hash))/ < $(location :extra_boot_args_template) > $@",
        )

        if upgrades:
            native.genrule(
                name = "partition-root-test-sign",
                srcs = ["partition-root-test-unsigned.tar"],
                outs = ["partition-root-test.tar", "partition-root-test-hash"],
                cmd = "$(location //toolchains/sysimage:verity_sign.py) -i $< -o $(location :partition-root-test.tar) -r $(location partition-root-test-hash)",
                tools = ["//toolchains/sysimage:verity_sign.py"],
            )

            native.genrule(
                name = "extra_boot_args_root_test_hash",
                srcs = [
                    ":extra_boot_args_template",
                    ":partition-root-test-hash",
                ],
                outs = ["extra_boot_test_args"],
                cmd = "sed -e s/ROOT_HASH/$$(cat $(location :partition-root-test-hash))/ < $(location :extra_boot_args_template) > $@",
            )

    ext4_image(
        name = "partition-boot.tar",
        src = _dict_value_search(image_deps["bootfs"], "/"),
        # Take the dependency list declared above, and add in the "version.txt"
        # as well as the generated extra_boot_args file in the correct place.
        extra_files = {
            k: v
            for k, v in (
                image_deps["bootfs"].items() + [
                    ("version.txt", "/boot/version.txt:0644"),
                    ("extra_boot_args", "/boot/extra_boot_args:0644"),
                ]
            )
            # Skip over special entries
            if v != "/"
        },
        file_contexts = ":file_contexts",
        partition_size = image_deps["bootfs_size"],
        subdir = "boot/",
        target_compatible_with = [
            "@platforms//os:linux",
        ],
    )

    if upgrades:
        ext4_image(
            name = "partition-boot-test.tar",
            src = _dict_value_search(image_deps["rootfs"], "/"),
            # Take the dependency list declared above, and add in the "version.txt"
            # as well as the generated extra_boot_args file in the correct place.
            extra_files = {
                k: v
                for k, v in (
                    image_deps["bootfs"].items() + [
                        ("version-test.txt", "/boot/version.txt:0644"),
                        ("extra_boot_test_args", "/boot/extra_boot_args:0644"),
                    ]
                )
                # Skip over special entries
                if v != "/"
            },
            file_contexts = ":file_contexts",
            partition_size = image_deps["bootfs_size"],
            subdir = "boot/",
            target_compatible_with = [
                "@platforms//os:linux",
            ],
        )

    # -------------------- Assemble disk image --------------------

    custom_partitions = image_deps.get("custom_partitions", default = [])

    disk_image(
        name = "disk-img.tar",
        layout = image_deps["partition_table"],
        partitions = [
            ":partition-boot.tar",
            ":partition-root.tar",
        ] + custom_partitions,
        expanded_size = image_deps.get("expanded_size", default = None),
        # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
        # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
        tags = ["no-remote-cache"],
        target_compatible_with = [
            "@platforms//os:linux",
        ],
    )

    native.genrule(
        name = "disk-img.tar_zst",
        srcs = ["disk-img.tar"],
        outs = ["disk-img.tar.zst"],
        cmd = "zstd --threads=0 -10 -f -z $< -o $@",
        # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
        # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
        tags = ["no-remote-cache"],
    )

    sha256sum(
        name = "disk-img.tar.zst.sha256",
        srcs = [":disk-img.tar.zst"],
    )

    gzip_compress(
        name = "disk-img.tar.gz",
        srcs = ["disk-img.tar"],
        # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
        # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
        tags = ["no-remote-cache"],
        visibility = visibility,
    )

    sha256sum(
        name = "disk-img.tar.gz.sha256",
        srcs = [":disk-img.tar.gz"],
    )

    # -------------------- Assemble upgrade image --------------------

    if upgrades:
        upgrade_image(
            name = "update-img.tar",
            boot_partition = ":partition-boot.tar",
            root_partition = ":partition-root.tar",
            # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
            # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
            tags = ["no-remote-cache"],
            target_compatible_with = [
                "@platforms//os:linux",
            ],
            version_file = ":version.txt",
        )

        native.genrule(
            name = "update-img.tar_zst",
            srcs = ["update-img.tar"],
            outs = ["update-img.tar.zst"],
            cmd = "zstd --threads=0 -10 -f -z $< -o $@",
            # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
            # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
            tags = ["no-remote-cache"],
        )

        sha256sum(
            name = "update-img.tar.zst.sha256",
            srcs = [":update-img.tar.zst"],
        )

        gzip_compress(
            name = "update-img.tar.gz",
            srcs = ["update-img.tar"],
            # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
            # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
            tags = ["no-remote-cache"],
        )

        sha256sum(
            name = "update-img.tar.gz.sha256",
            srcs = [":update-img.tar.gz"],
        )

        upgrade_image(
            name = "update-img-test.tar",
            boot_partition = ":partition-boot-test.tar",
            root_partition = ":partition-root-test.tar",
            # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
            # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
            tags = ["no-remote-cache"],
            target_compatible_with = [
                "@platforms//os:linux",
            ],
            version_file = ":version-test.txt",
        )

        native.genrule(
            name = "update-img-test.tar_zst",
            srcs = ["update-img-test.tar"],
            outs = ["update-img-test.tar.zst"],
            cmd = "zstd --threads=0 -10 -f -z $< -o $@",
            # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
            # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
            tags = ["no-remote-cache"],
        )

        sha256sum(
            name = "update-img-test.tar.zst.sha256",
            srcs = [":update-img-test.tar.zst"],
        )

        gzip_compress(
            name = "update-img-test.tar.gz",
            srcs = ["update-img-test.tar"],
            # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
            # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
            tags = ["no-remote-cache"],
        )

        sha256sum(
            name = "update-img-test.tar.gz.sha256",
            srcs = [":update-img-test.tar.gz"],
        )

    # -------------------- Upload artifacts --------------------

    upload_suffix = ""
    if mode == "dev":
        upload_suffix = "-dev"
    if malicious:
        upload_suffix += "-malicious"

    upload_artifacts(
        name = "upload_disk-img",
        inputs = [
            ":disk-img.tar.zst",
            ":disk-img.tar.gz",
        ],
        remote_subdir = upload_prefix + "/disk-img" + upload_suffix,
    )

    output_files(
        name = "disk-img-url",
        target = ":upload_disk-img",
        basenames = ["upload_disk-img_disk-img.tar.zst.url"],
        tags = ["manual"],
    )

    if upgrades:
        upload_artifacts(
            name = "upload_update-img",
            inputs = [
                ":update-img.tar.zst",
                ":update-img.tar.gz",
                ":update-img-test.tar.zst",
                ":update-img-test.tar.gz",
            ],
            remote_subdir = upload_prefix + "/update-img" + upload_suffix,
        )

    # -------------------- Bazel ergonomics --------------------

    native.filegroup(
        name = "hash_and_upload_disk-img",
        srcs = [
            ":upload_disk-img",
            ":disk-img.tar.zst.sha256",
        ],
        visibility = ["//visibility:public"],
        tags = ["manual"],
    )

    if upgrades:
        native.filegroup(
            name = "hash_and_upload_update-img",
            srcs = [
                ":upload_update-img",
                ":update-img.tar.zst.sha256",
            ],
            visibility = ["//visibility:public"],
            tags = ["manual"],
        )

        native.filegroup(
            name = "hash_and_upload_update-img-test",
            srcs = [
                ":upload_update-img-test",
                ":update-img-test.tar.zst.sha256",
            ],
            visibility = ["//visibility:public"],
            tags = ["manual"],
        )

    if upgrades:
        upgrade_outputs = [
            ":update-img.tar.zst",
            ":update-img.tar.gz",
            ":update-img-test.tar.zst",
            ":update-img-test.tar.gz",
        ]
    else:
        upgrade_outputs = []

    native.filegroup(
        name = name,
        srcs = [
            ":disk-img.tar.zst",
            ":disk-img.tar.gz",
        ] + upgrade_outputs,
        visibility = visibility,
    )

    # -------------------- Vulnerability scanning --------------------

    if vuln_scan:
        native.sh_binary(
            name = "vuln-scan",
            srcs = ["//ic-os:vuln-scan.sh"],
            data = [
                "@trivy//:trivy",
                ":rootfs-tree.tar",
                "//ic-os:vuln-scan.html",
            ],
            env = {
                "trivy_path": "$(rootpath @trivy//:trivy)",
                "DOCKER_TAR": "$(rootpaths :rootfs-tree.tar)",
                "TEMPLATE_FILE": "$(rootpath //ic-os:vuln-scan.html)",
            },
        )

def boundary_node_icos_build(name, image_deps, mode = None, sev = False, visibility = None):
    """
    A boundary node ICOS build parameterized by mode.

    Args:
      name: Name for the generated filegroup.
      image_deps: Function to be used to generate image manifest
      mode: dev, or prod. If not specified, will use the value of `name`
      sev: if True, build an SEV-SNP enabled image
      visibility: See Bazel documentation
    """
    if mode == None:
        mode = name

    image_deps = image_deps(mode, sev = sev)

    rootfs_args = []

    if mode == "dev":
        rootfs_args = [
            "ROOT_PASSWORD=root",
            "SW=false",
        ]
    elif mode == "prod":
        rootfs_args = [
            "ROOT_PASSWORD=",
            "SW=true",
        ]

    if sev:
        base_suffix = "snp"
    else:
        base_suffix = "prod"
    file_build_args = {"//ic-os/boundary-guestos:rootfs/docker-base." + base_suffix: "BASE_IMAGE"}

    native.sh_binary(
        name = "vuln-scan",
        srcs = ["//ic-os:vuln-scan.sh"],
        data = [
            "@trivy//:trivy",
            ":rootfs-tree.tar",
            "//ic-os:vuln-scan.html",
        ],
        env = {
            "trivy_path": "$(rootpath @trivy//:trivy)",
            "DOCKER_TAR": "$(rootpaths :rootfs-tree.tar)",
            "TEMPLATE_FILE": "$(rootpath //ic-os:vuln-scan.html)",
        },
    )

    docker_tar(
        visibility = visibility,
        name = "rootfs-tree.tar",
        dep = ["//ic-os/boundary-guestos:rootfs-files"],
        build_args = [
            "BUILD_TYPE=" + mode,
        ] + rootfs_args,
        file_build_args = file_build_args,
        # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
        # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
        tags = ["no-remote-cache"],
        target_compatible_with = [
            "@platforms//os:linux",
        ],
    )

    ext4_image(
        name = "partition-config.tar",
        partition_size = "100M",
        target_compatible_with = [
            "@platforms//os:linux",
        ],
    )

    # TODO(IDX-2538): re-enable this (or any other similar) solution when everything will be ready to have ic version that is not git revision.
    #summary_sha256sum(
    #    name = "version.txt",
    #    inputs = image_deps,
    #    suffix = "-dev" if mode == "dev" else "",
    #)

    copy_file(
        name = "copy_version_txt",
        src = "//bazel:version.txt",
        out = "version.txt",
        allow_symlink = True,
    )

    copy_file(
        name = "copy_ic_version_id",
        src = ":version.txt",
        out = "ic_version_id",
        allow_symlink = True,
        visibility = ["//visibility:public"],
        tags = ["manual"],
    )

    ext4_image(
        name = "partition-boot.tar",
        src = _dict_value_search(image_deps["rootfs"], "/"),
        # Take the dependency list declared above, and add in the "version.txt"
        # as well as the generated extra_boot_args file in the correct place.
        extra_files = {
            k: v
            for k, v in (
                image_deps["bootfs"].items() + [
                    ("version.txt", "/boot/version.txt:0644"),
                    ("extra_boot_args", "/boot/extra_boot_args:0644"),
                ]
            )
            # Skip over special entries
            if ":bootloader/extra_boot_args.template" not in k
            if v != "/"
        },
        partition_size = "1G",
        subdir = "boot/",
        target_compatible_with = [
            "@platforms//os:linux",
        ],
    )

    ext4_image(
        name = "partition-root-unsigned.tar",
        src = _dict_value_search(image_deps["rootfs"], "/"),
        # Take the dependency list declared above, and add in the "version.txt"
        # at the correct place.
        extra_files = {
            k: v
            for k, v in (image_deps["rootfs"].items() + [(":version.txt", "/opt/ic/share/version.txt:0644")])
            # Skip over special entries
            if v != "/"
        },
        partition_size = "3G",
        strip_paths = [
            "/run",
            "/boot",
        ],
        # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
        # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
        tags = ["no-remote-cache"],
        target_compatible_with = [
            "@platforms//os:linux",
        ],
    )

    native.genrule(
        name = "partition-root-sign",
        srcs = ["partition-root-unsigned.tar"],
        outs = ["partition-root.tar", "partition-root-hash"],
        cmd = "$(location //toolchains/sysimage:verity_sign.py) -i $< -o $(location :partition-root.tar) -r $(location partition-root-hash)",
        executable = False,
        tools = ["//toolchains/sysimage:verity_sign.py"],
    )

    native.genrule(
        name = "extra_boot_args_root_hash",
        srcs = [
            "//ic-os/boundary-guestos:bootloader/extra_boot_args.template",
            ":partition-root-hash",
        ],
        outs = ["extra_boot_args"],
        cmd = "sed -e s/ROOT_HASH/$$(cat $(location :partition-root-hash))/ < $(location //ic-os/boundary-guestos:bootloader/extra_boot_args.template) > $@",
    )

    disk_image(
        name = "disk-img.tar",
        layout = "//ic-os/boundary-guestos:partitions.csv",
        partitions = [
            "//ic-os/bootloader:partition-esp.tar",
            "//ic-os/bootloader:partition-grub.tar",
            ":partition-config.tar",
            ":partition-boot.tar",
            ":partition-root.tar",
        ],
        expanded_size = "50G",
        # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
        # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
        tags = ["no-remote-cache"],
        target_compatible_with = [
            "@platforms//os:linux",
        ],
    )

    native.genrule(
        name = "disk-img.tar_zst",
        srcs = ["disk-img.tar"],
        outs = ["disk-img.tar.zst"],
        cmd = "zstd --threads=0 -10 -f -z $< -o $@",
        # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
        # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
        tags = ["no-remote-cache"],
    )

    sha256sum(
        name = "disk-img.tar.zst.sha256",
        srcs = [":disk-img.tar.zst"],
    )

    gzip_compress(
        name = "disk-img.tar.gz",
        srcs = ["disk-img.tar"],
        # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
        # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
        tags = ["no-remote-cache"],
    )

    upload_suffix = ""
    if sev:
        upload_suffix += "-snp"
    if mode == "dev":
        upload_suffix += "-dev"

    upload_artifacts(
        name = "upload_disk-img",
        inputs = [
            ":disk-img.tar.zst",
            ":disk-img.tar.gz",
        ],
        remote_subdir = "boundary-os/disk-img" + upload_suffix,
    )

    native.filegroup(
        name = "hash_and_upload_disk-img",
        srcs = [
            ":upload_disk-img",
            ":disk-img.tar.zst.sha256",
        ],
        visibility = ["//visibility:public"],
        tags = ["manual"],
    )

    output_files(
        name = "disk-img-url",
        target = ":upload_disk-img",
        basenames = ["upload_disk-img_disk-img.tar.zst.url"],
        tags = ["manual"],
    )

    native.filegroup(
        name = name,
        srcs = [":disk-img.tar.zst", ":disk-img.tar.gz"],
        visibility = visibility,
    )

def boundary_api_guestos_build(name, image_deps, mode = None, visibility = None):
    """
    A boundary API GuestOS build parameterized by mode.

    Args:
      name: Name for the generated filegroup.
      image_deps: Function to be used to generate image manifest
      mode: dev, or prod. If not specified, will use the value of `name`
      visibility: See Bazel documentation
    """
    if mode == None:
        mode = name

    image_deps = image_deps()

    rootfs_args = []

    if mode == "dev":
        rootfs_args = [
            "ROOT_PASSWORD=root",
        ]
    elif mode == "prod":
        rootfs_args = [
            "ROOT_PASSWORD=",
        ]

    native.sh_binary(
        name = "vuln-scan",
        srcs = ["//ic-os:vuln-scan.sh"],
        data = [
            "@trivy//:trivy",
            ":rootfs-tree.tar",
            "//ic-os:vuln-scan.html",
        ],
        env = {
            "trivy_path": "$(rootpath @trivy//:trivy)",
            "DOCKER_TAR": "$(rootpaths :rootfs-tree.tar)",
            "TEMPLATE_FILE": "$(rootpath //ic-os:vuln-scan.html)",
        },
    )

    docker_tar(
        visibility = visibility,
        name = "rootfs-tree.tar",
        dep = ["//ic-os/boundary-api-guestos:rootfs-files"],
        build_args = [
            "BUILD_TYPE=" + mode,
        ] + rootfs_args,
        file_build_args = {
            "//ic-os/boundary-api-guestos:rootfs/docker-base.prod": "BASE_IMAGE",
        },
        # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
        # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
        tags = ["no-remote-cache"],
        target_compatible_with = [
            "@platforms//os:linux",
        ],
    )

    ext4_image(
        name = "partition-config.tar",
        partition_size = "100M",
        target_compatible_with = [
            "@platforms//os:linux",
        ],
    )

    # TODO(IDX-2538): re-enable this (or any other similar) solution when everything will be ready to have ic version that is not git revision.
    #summary_sha256sum(
    #    name = "version.txt",
    #    inputs = image_deps,
    #    suffix = "-dev" if mode == "dev" else "",
    #)

    copy_file(
        name = "copy_version_txt",
        src = "//bazel:version.txt",
        out = "version.txt",
        allow_symlink = True,
    )

    copy_file(
        name = "copy_ic_version_id",
        src = ":version.txt",
        out = "ic_version_id",
        allow_symlink = True,
        visibility = ["//visibility:public"],
        tags = ["manual"],
    )

    ext4_image(
        name = "partition-boot.tar",
        src = _dict_value_search(image_deps["rootfs"], "/"),
        # Take the dependency list declared above, and add in the "version.txt"
        # as well as the generated extra_boot_args file in the correct place.
        extra_files = {
            k: v
            for k, v in (
                image_deps["bootfs"].items() + [
                    ("version.txt", "/boot/version.txt:0644"),
                    ("extra_boot_args", "/boot/extra_boot_args:0644"),
                ]
            )
            # Skip over special entries
            if ":bootloader/extra_boot_args.template" not in k
            if v != "/"
        },
        partition_size = "1G",
        subdir = "boot/",
        target_compatible_with = [
            "@platforms//os:linux",
        ],
    )

    ext4_image(
        name = "partition-root-unsigned.tar",
        src = _dict_value_search(image_deps["rootfs"], "/"),
        # Take the dependency list declared above, and add in the "version.txt"
        # at the correct place.
        extra_files = {
            k: v
            for k, v in (image_deps["rootfs"].items() + [(":version.txt", "/opt/ic/share/version.txt:0644")])
            # Skip over special entries
            if v != "/"
        },
        partition_size = "3G",
        strip_paths = [
            "/run",
            "/boot",
        ],
        # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
        # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
        tags = ["no-remote-cache"],
        target_compatible_with = [
            "@platforms//os:linux",
        ],
    )

    native.genrule(
        name = "partition-root-sign",
        srcs = ["partition-root-unsigned.tar"],
        outs = ["partition-root.tar", "partition-root-hash"],
        cmd = "$(location //toolchains/sysimage:verity_sign.py) -i $< -o $(location :partition-root.tar) -r $(location partition-root-hash)",
        executable = False,
        tools = ["//toolchains/sysimage:verity_sign.py"],
    )

    native.genrule(
        name = "extra_boot_args_root_hash",
        srcs = [
            "//ic-os/boundary-api-guestos:bootloader/extra_boot_args.template",
            ":partition-root-hash",
        ],
        outs = ["extra_boot_args"],
        cmd = "sed -e s/ROOT_HASH/$$(cat $(location :partition-root-hash))/ < $(location //ic-os/boundary-api-guestos:bootloader/extra_boot_args.template) > $@",
    )

    disk_image(
        name = "disk-img.tar",
        layout = "//ic-os/boundary-api-guestos:partitions.csv",
        partitions = [
            "//ic-os/bootloader:partition-esp.tar",
            "//ic-os/bootloader:partition-grub.tar",
            ":partition-config.tar",
            ":partition-boot.tar",
            "partition-root.tar",
        ],
        expanded_size = "50G",
        # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
        # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
        tags = ["no-remote-cache"],
        target_compatible_with = [
            "@platforms//os:linux",
        ],
    )

    native.genrule(
        name = "disk-img.tar_zst",
        srcs = ["disk-img.tar"],
        outs = ["disk-img.tar.zst"],
        cmd = "zstd --threads=0 -10 -f -z $< -o $@",
        # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
        # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
        tags = ["no-remote-cache"],
    )

    sha256sum(
        name = "disk-img.tar.zst.sha256",
        srcs = [":disk-img.tar.zst"],
    )

    gzip_compress(
        name = "disk-img.tar.gz",
        srcs = ["disk-img.tar"],
        # The image is pretty big, therefore it is usually much faster to just rebuild it instead of fetching from the cache.
        # TODO(IDX-2221): remove this when CI jobs and bazel infrastructure will run in the same clusters.
        tags = ["no-remote-cache"],
    )

    upload_suffix = ""
    if mode == "dev":
        upload_suffix += "-dev"

    upload_artifacts(
        name = "upload_disk-img",
        inputs = [
            ":disk-img.tar.zst",
            ":disk-img.tar.gz",
        ],
        remote_subdir = "boundary-api-os/disk-img" + upload_suffix,
    )

    native.filegroup(
        name = "hash_and_upload_disk-img",
        srcs = [
            ":upload_disk-img",
            ":disk-img.tar.zst.sha256",
        ],
        visibility = ["//visibility:public"],
        tags = ["manual"],
    )

    output_files(
        name = "disk-img-url",
        target = ":upload_disk-img",
        basenames = ["upload_disk-img_disk-img.tar.zst.url"],
        tags = ["manual"],
    )

    native.filegroup(
        name = name,
        srcs = [":disk-img.tar.zst", ":disk-img.tar.gz"],
        visibility = visibility,
    )

# NOTE: Really, we should be using a string keyed label dict, but this is not
# a built in. Use this hack until I switch our implementation.
def _dict_value_search(dict, value):
    for k, v in dict.items():
        if v == value:
            return k

    return None
