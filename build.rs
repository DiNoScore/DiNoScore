use gtk4::gio;

fn main() {
	gio::compile_resources(
		"res/viewer",                         // Dir
		"res/viewer/resources.gresource.xml", // Index
		"viewer.gresource",                   // Name
	);
	gio::compile_resources(
		"res/editor",
		"res/editor/resources.gresource.xml",
		"editor.gresource",
	);
}
