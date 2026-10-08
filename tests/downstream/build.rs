fn main() {
	println!("cargo:rerun-if-changed=shader.slang");
	let global = shader_slang::GlobalSession::new().unwrap();
	let targets = [shader_slang::TargetDesc::default()
		.format(shader_slang::CompileTarget::Spirv)
		.profile(global.find_profile("glsl_450"))];
	let search_path = std::ffi::CString::new(env!("CARGO_MANIFEST_DIR")).unwrap();
	let paths = [search_path.as_ptr()];
	let desc = shader_slang::SessionDesc::default()
		.targets(&targets)
		.search_paths(&paths);
	let session = global.create_session(&desc).unwrap();
	let module = session.load_module("shader.slang").unwrap();
	let entry = module.find_entry_point_by_name("main").unwrap();
	let program = session
		.create_composite_component_type(&[module.into(), entry.into()])
		.unwrap();
	let linked = program.link().unwrap();
	assert!(!linked.entry_point_code(0, 0).unwrap().as_slice().is_empty());
}
