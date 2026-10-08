#!/usr/bin/env python3
"""Linux runtime regression: python3 tests/check_downstream.py (downloads Slang)."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "tests/downstream/Cargo.toml"
OVERRIDES = (
    "SLANG_DIR", "SLANG_INCLUDE_DIR", "SLANG_LIB_DIR", "SLANG_BIN_DIR",
    "VULKAN_SDK", "SLANG_NO_DOWNLOAD", "LD_LIBRARY_PATH", "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
)
ENV = {key: value for key, value in os.environ.items() if key not in OVERRIDES}


def cargo(target, *args, env=ENV):
    subprocess.run(
        ["cargo", *args, "--manifest-path", str(MANIFEST), "--target-dir", str(target)],
        env=env, check=True, cwd=ROOT,
    )


def check_repository_filesystem(build_script, install_root):
    # Cargo dependencies are compiled in /tmp for speed, but exercise staging
    # on the repository filesystem too (which may be ntfs3, unlike /tmp).
    parent = ROOT / "target"
    parent.mkdir(exist_ok=True)
    cfg = subprocess.check_output(["rustc", "--print", "cfg"], env=ENV, text=True)
    target_env = {
        "CARGO_CFG_" + key.upper(): value.strip('"')
        for line in cfg.splitlines() if "=" in line
        for key, value in [line.split("=", 1)]
    }
    with tempfile.TemporaryDirectory(prefix="slang-staging-", dir=parent) as temporary:
        profile = Path(temporary) / "debug"
        out = profile / "build/shader-slang-sys-fixture/out"
        root = out / install_root.name
        (root / "lib").mkdir(parents=True)
        (root / "include").symlink_to(install_root / "include", target_is_directory=True)
        # Staging itself does not load these files; genuine compiler execution
        # is tested by the downstream build script above.
        (root / "lib/libslang-compiler.so.0.fixture").write_bytes(b"compiler")
        (root / "lib/libslang-compiler.so").symlink_to("libslang-compiler.so.0.fixture")
        (root / "lib/libslang-backend.so").write_bytes(b"backend")
        expected = {path.name for path in (root / "lib").iterdir()}
        for _ in range(3):
            subprocess.run([str(build_script)], cwd=ROOT / "slang-sys",
                           env={**ENV, **target_env, "OUT_DIR": str(out)}, check=True,
                           stdout=subprocess.DEVNULL, timeout=30)
            entries = list((profile / "deps").iterdir())
            assert len(entries) == len(expected), entries
            assert {path.name for path in entries} == expected
            for path in entries:
                assert path.readlink() == out / path.name
                assert path.read_bytes() == (root / "lib" / path.name).read_bytes()


if len(sys.argv) == 4 and sys.argv[1] == "--staging-only":
    check_repository_filesystem(Path(sys.argv[2]), Path(sys.argv[3]))
    sys.exit(0)

with tempfile.TemporaryDirectory(prefix="slang-downstream-") as temporary:
    target = Path(temporary) / "target"
    cargo(target, "check")
    cargo(target, "check", "--all-targets")
    cargo(target, "check")
    # Force another execution, rather than merely checking a fresh fingerprint.
    cargo(target, "clean", "-p", "slang-downstream-build-test")
    cargo(target, "check", "--all-targets", "--message-format=json")

    install_root = next((target / "debug/build").glob("shader-slang-sys-*/out/slang-*/lib")).parent
    out = install_root.parent
    assert (out / "libslang-compiler.so").is_file()
    assert any(out.glob("libslang-compiler.so.*"))
    assert all((out / path.name).is_file() for path in (install_root / "lib").glob("*.so*"))
    build_script = next((target / "debug/build").glob("shader-slang-sys-*/build-script-build"))
    check_repository_filesystem(build_script, install_root)
    (out / "libslang-obsolete.so.0").write_text("stale version")
    (target / "debug/deps/libslang-obsolete.so.0").symlink_to(out / "libslang-obsolete.so.0")
    cargo(target, "check", env={**ENV, "SLANG_NO_DOWNLOAD": "0"})
    assert not (out / "libslang-obsolete.so.0").exists()
    assert not (target / "debug/deps/libslang-obsolete.so.0").is_symlink()

    # Explicit installations retain their external loader-path requirement.
    install = Path(temporary) / "explicit-slang"
    shutil.copytree(install_root, install, symlinks=True)
    explicit = {**ENV, "SLANG_DIR": str(install), "SLANG_NO_DOWNLOAD": "1",
                "LD_LIBRARY_PATH": str(install / "lib")}
    cargo(Path(temporary) / "explicit-target", "check", "--all-targets", env=explicit)
    cargo(target, "check", "--all-targets", env=explicit)
    assert not (out / "libslang-compiler.so").exists()
    assert not (target / "debug/deps/libslang-compiler.so").is_symlink()
    separate = {**ENV, "SLANG_INCLUDE_DIR": str(install / "include"),
                "SLANG_LIB_DIR": str(install / "lib"), "SLANG_NO_DOWNLOAD": "1",
                "LD_LIBRARY_PATH": str(install / "lib")}
    cargo(target, "check", env=separate)
    # Exercise Vulkan SDK layout discovery without requiring a full SDK.
    sdk = Path(temporary) / "vulkan-sdk-layout"
    (sdk / "include").mkdir(parents=True)
    (sdk / "include/slang").symlink_to(install / "include", target_is_directory=True)
    (sdk / "lib").symlink_to(install / "lib", target_is_directory=True)
    cargo(target, "check", env={**ENV, "VULKAN_SDK": str(sdk),
                              "SLANG_NO_DOWNLOAD": "1",
                              "LD_LIBRARY_PATH": str(sdk / "lib")})

    cargo(target, "clean")
    cargo(target, "check")
    cargo(target, "check", "--all-targets")
    cargo(target, "build")
