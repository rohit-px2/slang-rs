extern crate bindgen;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Slang release automatically provisioned when no system installation is found.
///
/// Keep this in sync with the hard-coded SHA-256 checksums in
/// `release_asset_for_target` below.
const SLANG_VERSION: &str = "2026.17";

/// A discovered or downloaded Slang installation.
///
/// Both go through exactly the same remaining build logic (link + bindgen).
struct SlangInstall {
	include_dir: PathBuf,
	lib_dir: PathBuf,
	/// Directory holding runtime shared libraries (`bin/` on Windows layouts).
	bin_dir: PathBuf,
}

fn main() {
	println!("cargo:rerun-if-env-changed=SLANG_DIR");
	println!("cargo:rerun-if-env-changed=SLANG_INCLUDE_DIR");
	println!("cargo:rerun-if-env-changed=SLANG_LIB_DIR");
	println!("cargo:rerun-if-env-changed=SLANG_BIN_DIR");
	println!("cargo:rerun-if-env-changed=VULKAN_SDK");
	println!("cargo:rerun-if-env-changed=SLANG_NO_DOWNLOAD");

	let install = find_slang().unwrap_or_else(|err| panic!("{err}"));

	let out_dir =
		PathBuf::from(env::var_os("OUT_DIR").expect("Couldn't determine output directory."));

	// Since Slang v2025.21 the primary compiler library is `slang-compiler`
	// (`slang-compiler.dll`, `libslang-compiler.so/.dylib`). The old `slang`
	// names only remain as compatibility aliases scheduled for removal at the
	// end of 2026, so prefer the new name and only fall back for older
	// system installations (e.g. older Vulkan SDKs).
	let lib_name = resolve_lib_name(&install.lib_dir);
	println!(
		"cargo:rustc-link-search=native={}",
		install.lib_dir.display()
	);
	println!("cargo:rustc-link-lib=dylib={lib_name}");

	// On Windows the import library lives in `lib/` but the DLLs live in
	// `bin/`. Copy them next to the build output so host-side users (build
	// scripts, tests, examples) can load them without extra setup. Shipped
	// applications using Slang at runtime must still deploy these DLLs
	// alongside their executable; see the crate README.
	if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
		copy_dlls_to_out_dir(&install.bin_dir, &out_dir);
		// Cargo adds native search paths inside OUT_DIR to the runtime search
		// path when launching dependent build scripts and other host-side users.
		println!("cargo:rustc-link-search=native={}", out_dir.display());
	}

	let header = install.include_dir.join("slang.h");
	bindgen::builder()
		.header(header.to_string_lossy())
		.clang_arg("-v")
		.clang_arg("-xc++")
		.clang_arg("-std=c++17")
		.allowlist_function("spReflection.*")
		.allowlist_function("spComputeStringHash")
		.allowlist_function("slang_.*")
		.allowlist_type("slang.*")
		.allowlist_var("SLANG_.*")
		.with_codegen_config(
			bindgen::CodegenConfig::FUNCTIONS
				| bindgen::CodegenConfig::TYPES
				| bindgen::CodegenConfig::VARS,
		)
		.parse_callbacks(Box::new(ParseCallback {}))
		.default_enum_style(bindgen::EnumVariation::Rust {
			non_exhaustive: false,
		})
		.constified_enum("SlangProfileID")
		.constified_enum("SlangCapabilityID")
		.vtable_generation(true)
		.layout_tests(false)
		.derive_copy(true)
		.generate()
		.expect("Couldn't generate bindings.")
		.write_to_file(out_dir.join("bindings.rs"))
		.expect("Couldn't write bindings.");
}

fn find_slang() -> Result<SlangInstall, String> {
	if let Some(install) = find_explicit_slang()? {
		return Ok(install);
	}

	if let Some(install) = find_vulkan_sdk() {
		return Ok(install);
	}

	download_slang_prebuilt()
}

/// `SLANG_INCLUDE_DIR` + `SLANG_LIB_DIR`, falling back to `SLANG_DIR`.
fn find_explicit_slang() -> Result<Option<SlangInstall>, String> {
	let include_env = env::var("SLANG_INCLUDE_DIR").ok();
	let lib_env = env::var("SLANG_LIB_DIR").ok();
	let dir_env = env::var("SLANG_DIR").ok();

	if include_env.is_none() && lib_env.is_none() && dir_env.is_none() {
		return Ok(None);
	}

	let include_dir = include_env
		.or_else(|| dir_env.as_ref().map(|dir| format!("{dir}/include")))
		.ok_or_else(|| {
			"SLANG_LIB_DIR is set but neither SLANG_INCLUDE_DIR nor SLANG_DIR is set".to_string()
		})?;
	let lib_dir = lib_env
		.or_else(|| dir_env.as_ref().map(|dir| format!("{dir}/lib")))
		.ok_or_else(|| {
			"SLANG_INCLUDE_DIR is set but neither SLANG_LIB_DIR nor SLANG_DIR is set".to_string()
		})?;

	if !Path::new(&include_dir).join("slang.h").exists() {
		return Err(format!(
			"Slang headers not found: `{include_dir}/slang.h` does not exist. \
			Check SLANG_INCLUDE_DIR / SLANG_DIR."
		));
	}

	let bin_dir = env::var("SLANG_BIN_DIR")
		.ok()
		.or_else(|| dir_env.as_ref().map(|dir| format!("{dir}/bin")))
		.unwrap_or_else(|| lib_dir.clone());

	Ok(Some(SlangInstall {
		include_dir: PathBuf::from(include_dir),
		lib_dir: PathBuf::from(lib_dir),
		bin_dir: PathBuf::from(bin_dir),
	}))
}

/// `VULKAN_SDK` (the LunarG SDK bundles Slang).
fn find_vulkan_sdk() -> Option<SlangInstall> {
	let sdk = env::var("VULKAN_SDK").ok()?;
	let include_dir = PathBuf::from(format!("{sdk}/include/slang"));
	let lib_dir = PathBuf::from(format!("{sdk}/lib"));

	if !include_dir.join("slang.h").exists() {
		println!(
			"cargo:warning=VULKAN_SDK is set but `{}/slang.h` was not found; falling back to {}",
			include_dir.display(),
			fallback_description(),
		);
		return None;
	}

	Some(SlangInstall {
		include_dir,
		bin_dir: lib_dir.clone(),
		lib_dir,
	})
}

fn fallback_description() -> &'static str {
	#[cfg(feature = "download-slang")]
	return "the automatically downloaded Slang";
	#[cfg(not(feature = "download-slang"))]
	return "an error (enable the `download-slang` feature for automatic provisioning)";
}

/// Pick the link library name based on which files are present.
///
/// The `slang-compiler` name is used since Slang v2025.21; older
/// installations only ship the legacy `slang` name.
fn resolve_lib_name(lib_dir: &Path) -> &'static str {
	let entries: Vec<String> = fs::read_dir(lib_dir)
		.map(|entries| {
			entries
				.filter_map(|entry| entry.ok())
				.filter_map(|entry| entry.file_name().into_string().ok())
				.collect()
		})
		.unwrap_or_default();

	let has_new = entries.iter().any(|name| {
		name == "slang-compiler.lib"
			|| name.starts_with("libslang-compiler.")
			|| name.starts_with("libslang-compiler_")
	});
	if has_new {
		return "slang-compiler";
	}

	let has_legacy = entries.iter().any(|name| {
		name == "slang.lib" || name.starts_with("libslang.") || name.starts_with("libslang_")
	});
	if has_legacy {
		println!(
			"cargo:warning=linking against the legacy `slang` library name; \
			Slang renamed it to `slang-compiler` in v2025.21 and the old name \
			will be removed at the end of 2026. Consider updating your Slang installation."
		);
		return "slang";
	}

	// Unknown layout: assume the new name (correct for all current releases)
	// and let the linker report the details if it is wrong.
	"slang-compiler"
}

/// Copy `*.dll` next to the build output (best effort, Windows only).
fn copy_dlls_to_out_dir(bin_dir: &Path, out_dir: &Path) {
	let entries = match fs::read_dir(bin_dir) {
		Ok(entries) => entries,
		Err(_) => return,
	};
	for entry in entries.filter_map(|entry| entry.ok()) {
		let path = entry.path();
		if path.extension().and_then(|ext| ext.to_str()) != Some("dll") {
			continue;
		}
		if let Some(name) = path.file_name() {
			// Missing DLLs must not fail the build (e.g. minimal layouts);
			// linking would already have failed earlier in that case.
			let _ = fs::copy(&path, out_dir.join(name));
		}
	}
}

#[cfg(feature = "download-slang")]
fn download_slang_prebuilt() -> Result<SlangInstall, String> {
	if downloads_disabled() {
		return Err(format!(
			"No Slang installation found (SLANG_INCLUDE_DIR, SLANG_DIR and VULKAN_SDK are all unset) \
			and automatic downloading is disabled via SLANG_NO_DOWNLOAD. \
			Install Slang (e.g. the LunarG Vulkan SDK or https://github.com/shader-slang/slang/releases, \
			pinned version for this crate: {SLANG_VERSION}) and point SLANG_DIR at it."
		));
	}

	let asset = release_asset_for_target()?;

	let out_dir =
		PathBuf::from(env::var_os("OUT_DIR").expect("Couldn't determine output directory."));
	let root = out_dir.join(format!("slang-{SLANG_VERSION}"));

	if !root.join("include/slang.h").exists() {
		// Stale or interrupted extraction: start over.
		let _ = fs::remove_dir_all(&root);
		let archive_path = out_dir.join(&asset.file_name);
		download_asset(&asset, &archive_path)?;
		verify_sha256(&archive_path, asset.sha256)?;
		extract_zip(&archive_path, &root)?;
		// Keep the archive for reproducible rebuilds, but never fail if it
		// cannot be removed.
		let _ = fs::remove_file(&archive_path);

		if !root.join("include/slang.h").exists() {
			return Err(format!(
				"Downloaded {} but `include/slang.h` is still missing after extraction.",
				asset.file_name
			));
		}
	}

	// Official packages keep headers in `include/`, import libraries in
	// `lib/` and Windows runtime DLLs in `bin/`.
	let lib_dir = root.join("lib");
	let bin_dir = {
		let bin = root.join("bin");
		if bin.is_dir() { bin } else { lib_dir.clone() }
	};
	Ok(SlangInstall {
		include_dir: root.join("include"),
		lib_dir,
		bin_dir,
	})
}

#[cfg(not(feature = "download-slang"))]
fn download_slang_prebuilt() -> Result<SlangInstall, String> {
	Err("No Slang installation found. Set SLANG_INCLUDE_DIR/SLANG_LIB_DIR, SLANG_DIR or VULKAN_SDK, \
		or enable the `download-slang` cargo feature to download a pinned prebuilt automatically."
		.to_string())
}

#[cfg(feature = "download-slang")]
fn downloads_disabled() -> bool {
	match env::var("SLANG_NO_DOWNLOAD") {
		Ok(value) => !value.is_empty() && value != "0",
		Err(_) => false,
	}
}

/// Map the Rust target to the corresponding official Slang release asset.
#[cfg(feature = "download-slang")]
fn release_asset_for_target() -> Result<ReleaseAsset, String> {
	let os =
		env::var("CARGO_CFG_TARGET_OS").map_err(|_| "Couldn't determine target OS.".to_string())?;
	let arch = env::var("CARGO_CFG_TARGET_ARCH")
		.map_err(|_| "Couldn't determine target architecture.".to_string())?;
	let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

	if os == "windows" && target_env != "msvc" {
		return Err(format!(
			"Automatic Slang downloads only support the MSVC ABI on Windows (target `{arch}-pc-windows-{target_env}`). \
			Install Slang manually and set SLANG_DIR, or use an -msvc target."
		));
	}

	let (platform, sha256) = match (os.as_str(), arch.as_str()) {
		("windows", "x86_64") => (
			"windows-x86_64",
			"bc8cf08b24aaf44d98f06b7d578d0557a21bfed1ce6bfb9765a0f0d21dec8f31",
		),
		("windows", "aarch64") => (
			"windows-aarch64",
			"f29fb5292e65ed5c1f4db73b0c4cad4dd8c0008613120379fe2db8ea86ce6e42",
		),
		("linux", "x86_64") => (
			"linux-x86_64",
			"a81aa4a7ff9292f08e9b6478284458c09baddf9d51e46726fbb8bcfcf18941a6",
		),
		("linux", "aarch64") => (
			"linux-aarch64",
			"cf6f4c25a7b639c7fe4a76c9a2baf1c9f29a6f814012c9ce83454c0871e48b74",
		),
		("macos", "x86_64") => (
			"macos-x86_64",
			"3a807d0fda43f1618f1b0fa3cca3279582d3413001b53e114290da7e3032033d",
		),
		("macos", "aarch64") => (
			"macos-aarch64",
			"8451d6898bd8ea4f5ded26e6a1f2a05f6524deb6c7fedd4d577cf23eea7dece6",
		),
		_ => {
			return Err(format!(
				"Automatic Slang downloads are not supported for target `{arch}-{os}`. \
				Install Slang from https://github.com/shader-slang/slang/releases and set SLANG_DIR."
			));
		}
	};

	let file_name = format!("slang-{SLANG_VERSION}-{platform}.zip");
	Ok(ReleaseAsset {
		url: format!(
			"https://github.com/shader-slang/slang/releases/download/v{SLANG_VERSION}/{file_name}"
		),
		file_name,
		sha256,
	})
}

#[cfg(feature = "download-slang")]
struct ReleaseAsset {
	file_name: String,
	sha256: &'static str,
	url: String,
}

#[cfg(feature = "download-slang")]
fn download_asset(asset: &ReleaseAsset, dest: &Path) -> Result<(), String> {
	use std::io::Read;
	use std::time::Duration;

	println!(
		"cargo:warning=downloading Slang {} from {}",
		asset.file_name, asset.url
	);

	let mut response = ureq::Agent::new_with_config(
		ureq::Agent::config_builder()
			.timeout_global(Some(Duration::from_secs(600)))
			.build(),
	)
	.get(&asset.url)
	.call()
	.map_err(|err| format!("Failed to download Slang from {}: {err}", asset.url))?;

	let body = response.body_mut();
	let mut reader = body.as_reader();
	let mut bytes = Vec::new();
	reader
		.read_to_end(&mut bytes)
		.map_err(|err| format!("Failed to read Slang download: {err}"))?;

	fs::write(dest, &bytes).map_err(|err| format!("Failed to write Slang archive: {err}"))?;
	Ok(())
}

#[cfg(feature = "download-slang")]
fn verify_sha256(archive: &Path, expected: &str) -> Result<(), String> {
	use sha2::{Digest, Sha256};

	let bytes = fs::read(archive).map_err(|err| format!("Failed to read Slang archive: {err}"))?;
	let mut hasher = Sha256::new();
	hasher.update(&bytes);
	let actual = format!("{:x}", hasher.finalize());

	if actual != expected {
		let _ = fs::remove_file(archive);
		return Err(format!(
			"SHA-256 mismatch for the downloaded Slang archive (expected {expected}, got {actual}). \
			The archive was deleted; refusing to build from untrusted binaries."
		));
	}
	Ok(())
}

/// Extract a Slang release zip, preserving symlinks and executable bits.
///
/// The Linux/macOS packages ship versioned shared libraries
/// (`libslang-compiler.so.0.<version>`) with `libslang-compiler.so`-style
/// symlinks pointing at them, so a naive extraction would produce broken
/// text files instead of links and the linker would fail.
#[cfg(feature = "download-slang")]
fn extract_zip(archive: &Path, root: &Path) -> Result<(), String> {
	use std::io::Read;

	let file =
		fs::File::open(archive).map_err(|err| format!("Failed to open Slang archive: {err}"))?;
	let mut zip =
		zip::ZipArchive::new(file).map_err(|err| format!("Failed to read Slang archive: {err}"))?;

	for i in 0..zip.len() {
		let mut entry = zip
			.by_index(i)
			.map_err(|err| format!("Failed to read Slang archive entry: {err}"))?;
		let Some(out_path) = entry.enclosed_name().map(|name| root.join(name)) else {
			return Err("Slang archive contains an unsafe entry name.".to_string());
		};

		if entry.is_dir() {
			fs::create_dir_all(&out_path)
				.map_err(|err| format!("Failed to extract Slang archive: {err}"))?;
			continue;
		}
		if let Some(parent) = out_path.parent() {
			fs::create_dir_all(parent)
				.map_err(|err| format!("Failed to extract Slang archive: {err}"))?;
		}

		#[cfg(unix)]
		if entry.unix_mode().is_some_and(is_symlink_mode) {
			let mut target = String::new();
			entry
				.read_to_string(&mut target)
				.map_err(|err| format!("Failed to extract Slang archive symlink: {err}"))?;
			drop(entry);
			let _ = fs::remove_file(&out_path);
			std::os::unix::fs::symlink(&target, &out_path)
				.map_err(|err| format!("Failed to extract Slang archive symlink: {err}"))?;
			continue;
		}

		let mode = entry.unix_mode();
		let mut out = fs::File::create(&out_path)
			.map_err(|err| format!("Failed to extract Slang archive: {err}"))?;
		std::io::copy(&mut entry, &mut out)
			.map_err(|err| format!("Failed to extract Slang archive: {err}"))?;
		drop(entry);

		#[cfg(unix)]
		if let Some(mode) = mode {
			use std::os::unix::fs::PermissionsExt;
			fs::set_permissions(&out_path, fs::Permissions::from_mode(mode & 0o777))
				.map_err(|err| format!("Failed to extract Slang archive: {err}"))?;
		}
	}
	Ok(())
}

#[cfg(all(feature = "download-slang", unix))]
fn is_symlink_mode(mode: u32) -> bool {
	mode & 0o170000 == 0o120000
}

#[derive(Debug)]
struct ParseCallback {}

impl bindgen::callbacks::ParseCallbacks for ParseCallback {
	fn enum_variant_name(
		&self,
		enum_name: Option<&str>,
		original_variant_name: &str,
		_variant_value: bindgen::callbacks::EnumVariantValue,
	) -> Option<String> {
		let enum_name = enum_name?;

		// Map enum names to the part of their variant names that needs to be trimmed.
		// When an enum name is not in this map the code below will try to trim the enum name itself.
		let mut map = std::collections::HashMap::new();
		map.insert("SlangMatrixLayoutMode", "SlangMatrixLayout");
		map.insert("SlangCompileTarget", "Slang");

		let trim = map.get(enum_name).unwrap_or(&enum_name);
		let new_variant_name = pascal_case_from_snake_case(original_variant_name);
		let new_variant_name = new_variant_name.trim_start_matches(trim);
		Some(new_variant_name.to_string())
	}

	#[cfg(feature = "serde")]
	fn add_derives(&self, info: &bindgen::callbacks::DeriveInfo<'_>) -> Vec<String> {
		if info.name.starts_with("Slang") && info.kind == bindgen::callbacks::TypeKind::Enum {
			return vec!["serde::Serialize".into(), "serde::Deserialize".into()];
		}
		vec![]
	}
}

/// Converts `snake_case` or `SNAKE_CASE` to `PascalCase`.
/// If the input is already in `PascalCase` it will be returned as is.
fn pascal_case_from_snake_case(snake_case: &str) -> String {
	let mut result = String::new();

	let should_lower = snake_case
		.chars()
		.filter(|c| c.is_alphabetic())
		.all(|c| c.is_uppercase());

	for part in snake_case.split('_') {
		for (i, c) in part.chars().enumerate() {
			if i == 0 {
				result.push(c.to_ascii_uppercase());
			} else if should_lower {
				result.push(c.to_ascii_lowercase());
			} else {
				result.push(c);
			}
		}
	}

	result
}
