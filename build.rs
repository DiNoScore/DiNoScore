use std::process::Command;

fn compile_blueprints(dir: &str) {
	let blp_files: Vec<_> = std::fs::read_dir(dir)
		.unwrap()
		.filter_map(|e| e.ok())
		.filter(|e| e.path().extension().is_some_and(|ext| ext == "blp"))
		.collect();

	for entry in &blp_files {
		let blp_path = entry.path();
		let ui_path = blp_path.with_extension("ui");

		println!("cargo:rerun-if-changed={}", blp_path.display());
		println!("cargo:rerun-if-changed={}", ui_path.display());

		let status = Command::new("blueprint-compiler")
			.args(["compile", "--output"])
			.arg(&ui_path)
			.arg(&blp_path)
			.status()
			.expect("Failed to run blueprint-compiler");

		if !status.success() {
			panic!(
				"blueprint-compiler failed for {}",
				blp_path.display()
			);
		}
	}
}

fn main() {
	/* Compile Blueprint files to UI XML */
	compile_blueprints("res/viewer");
	compile_blueprints("res/editor");

	/* Copy the icons we use from the Adwaita theme and vendor them in the binary, as fallback.
	 * Needless to say, the Adwaita icon theme needs to be installed on the system as build dependency.
	 */

	let xdg = xdg::BaseDirectories::with_prefix("icons");
	let theme = xdg.find_data_file("Adwaita/index.theme").unwrap();
	let theme = theme.parent().unwrap();
	glib_build_tools::compile_resources(
		&[theme.join("scalable"), theme.join("symbolic")],
		"res/icons/resources.gresource.xml",
		"icons.gresource",
	)
}
