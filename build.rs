fn main() {
	/* Copy the icons we use from the Adwaita theme and vendor them in the binary, as fallback.
	 * Needless to say, the Adwaita icon theme needs to be installed on the system as build dependency.
	 */

	let xdg = xdg::BaseDirectories::with_prefix("icons").unwrap();
	let theme = xdg.find_data_file("Adwaita/index.theme").unwrap();
	let theme = theme.parent().unwrap();
	glib_build_tools::compile_resources(
		&[theme.join("scalable"), theme.join("symbolic")],
		"res/icons/resources.gresource.xml",
		"icons.gresource",
	)
}
