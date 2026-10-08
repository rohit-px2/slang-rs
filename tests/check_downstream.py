#!/usr/bin/env python3
"""Linux runtime regression: python3 tests/check_downstream.py (downloads Slang)."""
import os
from pathlib import Path
import shutil
import subprocess
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
