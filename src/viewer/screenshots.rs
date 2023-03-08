//! Automatically take some screenshots of the software, disguised as tests

use super::*;
use std::env;
use test::*;

fn take_screenshot(path: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
	let screenshot = std::process::Command::new("grim")
		.args(["-"])
		.output()
		.context("Failed to start grim")?;
	if !screenshot.status.success() {
		anyhow::bail!(
			"Grim returned exit code {}: '{}'",
			screenshot.status.code().unwrap_or(-1),
			String::from_utf8_lossy(&screenshot.stderr)
		)
	}
	std::fs::write(path, &screenshot.stdout).context("Failed to write file")?;
	Ok(())
}

#[test]
#[ignore]
fn create_screenshots() -> anyhow::Result<()> {
	fern::Dispatch::new()
		.format(
			fern::formatter::FormatterBuilder::default()
				.color_config(|config| {
					config
						.debug(fern::colors::Color::Magenta)
						.trace(fern::colors::Color::BrightMagenta)
				})
				.build(),
		)
		.level(log::LevelFilter::Trace)
		.level_for("multipart", log::LevelFilter::Info)
		.level_for("serde_xml_rs", log::LevelFilter::Info)
		.chain(fern::logger::stdout())
		.apply()
		.context("Failed to initialize logger")?;

	/* Some primitive isolation */
	let tmp_dir = tempdir::TempDir::new("dinoscore-screenshots")?;
	env::set_var("XDG_DATA_HOME", tmp_dir.path());
	env::set_var("XDG_CONFIG_HOME", tmp_dir.path());
	env::set_var("XDG_CACHE_HOME", tmp_dir.path());
	/* Use mock library in the resources */
	env::set_var(
		"XDG_DATA_DIRS",
		format!(
			"{}:{}",
			env::current_dir().unwrap().join("test").display(),
			env::var("XDG_DATA_DIRS").unwrap()
		),
	);

	/* Start headless sway */
	env::set_var("WLR_BACKENDS", "headless");
	let mut sway = std::process::Command::new("sway")
		.args(["-c", "/dev/null", "--verbose"])
		.spawn()
		.context("Failed to start sway")?;
	std::thread::sleep(std::time::Duration::from_secs(1));

	/* Force the correct wayland display */
	env::set_var("GDK_BACKEND", "wayland");
	env::set_var("WAYLAND_DISPLAY", "wayland-1");

	pipeline::pipe! {
		gvdb_macros::include_gresource_from_xml!("res/viewer/resources.gresource.xml")
		=> glib::Bytes::from_static
		=> &gio::Resource::from_data(&_)?
		=> gio::resources_register
	};
	/* Vendor icons */
	gio::resources_register_include!("icons.gresource").context("Failed to register resources.")?;

	let application = gtk::Application::builder()
		.application_id("de.piegames.dinoscore.viewer")
		.flags(gio::ApplicationFlags::NON_UNIQUE)
		.resource_base_path("/de/piegames/dinoscore")
		.build();

	application.connect_startup(gtk_init);

	let runner = |window: window::Window| async move {
		/* Set things up */
		let theme = adw::StyleManager::default();
		theme.set_color_scheme(adw::ColorScheme::ForceLight);
		window.present();
		window.queue_draw();
		window.fullscreen();
		glib::timeout_future(std::time::Duration::from_secs(10)).await;
		let library = window.library();
		let song = window.song();

		library.select_first_entry();
		yield_now().await;
		take_screenshot("gallery/01-overview.png")
			.context("Failed to take screenshot")
			.unwrap();

		library.activate_selected_entry(0);
		yield_now().await;
		/* Wait for the full resolution to load in background */
		glib::timeout_future(std::time::Duration::from_secs(10)).await;
		yield_now().await;
		take_screenshot("gallery/02-song.png")
			.context("Failed to take screenshot")
			.unwrap();

		song.part_selection().popup();
		yield_now().await;
		take_screenshot("gallery/03-parts.png")
			.context("Failed to take screenshot")
			.unwrap();
		song.part_selection().popdown();
		yield_now().await;

		song.set_zoom_mode("fit-staves");
		yield_now().await;
		song.zoom_button().activate();
		yield_now().await;
		take_screenshot("gallery/04-zoom.png")
			.context("Failed to take screenshot")
			.unwrap();
		song.zoom_button().activate();
		yield_now().await;

		theme.set_color_scheme(adw::ColorScheme::ForceDark);
		yield_now().await;
		take_screenshot("gallery/05-dark.png")
			.context("Failed to take screenshot")
			.unwrap();

		window.close();
	};

	application.connect_activate(move |application| {
		let window = window::Window::new(application);
		glib::MainContext::default().spawn_local_with_priority(glib::PRIORITY_LOW, runner(window));
	});

	application.run_with_args(&[] as &[&str]);

	sway.kill()?;
	sway.wait()?;

	Ok(())
}
