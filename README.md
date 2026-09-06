<div align="center">

# shader-slang
**Rust bindings for the [Slang](https://github.com/shader-slang/slang/) shader language compiler**

</div>

Supports both the modern compilation and reflection API.

Currently mostly reflects the needs of our own [engine](https://github.com/FloatyMonkey/engine) but contributions are more than welcome.

## Example

```rust
let global_session = slang::GlobalSession::new().unwrap();

let search_path = std::ffi::CString::new("shaders/directory").unwrap();

// All compiler options are available through this builder.
let session_options = slang::CompilerOptions::default()
	.optimization(slang::OptimizationLevel::High)
	.matrix_layout_row(true);

let target_desc = slang::TargetDesc::default()
	.format(slang::CompileTarget::Spirv)
	.profile(global_session.find_profile("glsl_450"));

let targets = [target_desc];
let search_paths = [search_path.as_ptr()];

let session_desc = slang::SessionDesc::default()
	.targets(&targets)
	.search_paths(&search_paths)
	.options(&session_options);

let session = global_session.create_session(&session_desc).unwrap();
let module = session.load_module("filename.slang").unwrap();
let entry_point = module.find_entry_point_by_name("main").unwrap();

let program = session
	.create_composite_component_type(&[module.into(), entry_point.into()])
	.unwrap();

let linked_program = program.link().unwrap();

// Entry point to the reflection API.
let reflection = linked_program.layout(0).unwrap();

let shader_bytecode = linked_program.entry_point_code(0, 0).unwrap();
```

## Installation

Add `shader-slang` to the `[dependencies]` section of your `Cargo.toml`.

By default no Slang installation is required: if no system Slang is found,
the build script downloads a pinned official Slang release (currently
`2026.17`) for your target, verifies its SHA-256 checksum, and builds against
it. The download is cached in Cargo's `OUT_DIR`, so it normally happens once
(`cargo clean` removes it and triggers a re-download). Supported targets are
Windows (MSVC), Linux (glibc) and macOS, on x86-64 and ARM64.

To use a system Slang instead, set one of the following before building
(first match wins):

| Environment variable(s) | Meaning |
| --- | --- |
| `SLANG_INCLUDE_DIR` + `SLANG_LIB_DIR` | Include and library directories separately (`SLANG_BIN_DIR` optionally points at the Windows DLLs) |
| `SLANG_DIR` | Slang installation root (expects `include/`, `lib/` and, on Windows, `bin/` underneath) |
| `VULKAN_SDK` | LunarG Vulkan SDK, which bundles the Slang compiler |

To opt out of the automatic download (offline, Nix, Bazel or vendored
builds), disable the `download-slang` cargo feature or set
`SLANG_NO_DOWNLOAD=1`:

```toml
[dependencies]
shader-slang = { version = "...", default-features = false }
```

An easy manual setup is installing the [LunarG Vulkan SDK](https://vulkan.lunarg.com),
which adds `VULKAN_SDK` to the environment automatically.

Alternatively, download Slang from their [releases page](https://github.com/shader-slang/slang/releases)
and set `SLANG_DIR` to the extracted directory.

### Runtime deployment

Automatic provisioning makes *build-time* Slang compilation (e.g. `.slang`
to SPIR-V in a build script) self-contained. Applications that use Slang
*at runtime* must still ship the Slang shared library with their executable:
on Windows copy `slang-compiler.dll` (plus `slang-rt.dll`/`slang-glslang.dll`
if used) next to the executable; on Linux/macOS make sure
`libslang-compiler.so`/`libslang-compiler.dylib` is on the loader search path.
To compile to DXIL bytecode, also copy `dxil.dll` and `dxcompiler.dll` from
the [Microsoft DirectXShaderCompiler](https://github.com/microsoft/DirectXShaderCompiler/releases)
to your executable's directory.

Since Slang v2025.21 the compiler library is named `slang-compiler`
(`slang-compiler.dll`, `libslang-compiler.so/.dylib`); this crate links
against that name and only falls back to the legacy `slang` name for older
system installations.

## Credits

Maintained by Lauro Oyen ([@laurooyen](https://github.com/laurooyen)).

Licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
