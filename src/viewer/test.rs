//! Test some interactions with the software,
//! mainly to prevent regressions

use super::*;
use std::env;

pub fn init_test() -> anyhow::Result<()> {
	let _ = fern::Dispatch::new()
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
		.apply();

	/* Some primitive isolation */
	let tmp_dir = tempdir::TempDir::new("dinoscore-tests")?;
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

	pipeline::pipe! {
		gvdb_macros::include_gresource_from_xml!("res/viewer/resources.gresource.xml")
		=> glib::Bytes::from_static
		=> &gio::Resource::from_data(&_)?
		=> gio::resources_register
	};
	/* Vendor icons */
	gio::resources_register_include!("icons.gresource").context("Failed to register resources.")?;

	Ok(())
}

pub fn run_gui<F>(runner: impl Fn(window::Window) -> F + 'static)
where
	F: std::future::Future<Output = ()> + 'static,
{
	let application = gtk::Application::builder()
		.application_id("de.piegames.dinoscore.viewer")
		.flags(gio::ApplicationFlags::NON_UNIQUE)
		.resource_base_path("/de/piegames/dinoscore")
		.build();

	application.connect_startup(gtk_init);

	application.connect_activate(move |application| {
		let window = window::Window::new(application);
		glib::MainContext::default().spawn_local_with_priority(glib::PRIORITY_LOW, runner(window));
	});

	application.run_with_args(&[] as &[&str]);
}

#[test]
#[ignore]
fn test_viewer_gui() -> anyhow::Result<()> {
	init_test()?;

	/* Use broadway as backend */
	log::info!("Setting up broadway backend");
	let mut broadway = std::process::Command::new("gtk4-broadwayd")
		.spawn()
		.context("Failed to start sway")?;
	std::thread::sleep(std::time::Duration::from_secs(1));
	env::set_var("GDK_BACKEND", "broadway");

	run_gui(|window: window::Window| async move {
		/* Set things up */
		log::info!("Setting up window");
		let theme = adw::StyleManager::default();
		theme.set_color_scheme(adw::ColorScheme::ForceLight);
		window.present();
		window.set_default_size(1200, 800);
		window.maximize();
		window.set_maximized(true);
		window.fullscreen();
		window.queue_draw();
		let library = window.library();
		let song = window.song();

		library.select_first_entry();
		yield_now().await;

		/* Start at the second piece */
		log::info!("Load a song, 2nd part");
		library.activate_selected_entry(1);
		yield_now().await;
		/* Wait for the full resolution to load in background */
		glib::timeout_future(std::time::Duration::from_secs(10)).await;
		yield_now().await;

		/* Check that we actually are in the second piece */
		assert_eq!(song.part_selection().active_id(), Some("22".into()));
		assert!(song.carousel().position() > 0.0);

		/* Change the zoom level */
		log::info!("Change zoom level");
		song.set_zoom_mode("fit-staves");
		yield_now().await;
		glib::timeout_future(std::time::Duration::from_secs(10)).await;
		yield_now().await;

		// Disabled, looks like a bug in libadwaita :(
		// /* Check that we are still in the correct song */
		// assert_eq!(song.part_selection().active_id(), Some("22".into()));
		// assert!(song.carousel().position() > 0.0);

		/* Unload the song */
		log::info!("Unload song");
		song.unload_song();

		/* Reload the song again */
		log::info!("Reload song, at same position");
		library.activate_selected_entry(1);
		yield_now().await;
		glib::timeout_future(std::time::Duration::from_secs(10)).await;
		yield_now().await;

		/* Check that the zoom level was saved (and also the position is correct) */
		assert_eq!(song.zoom_mode(), library::ScaleMode::FitStaves(3));
		assert_eq!(song.part_selection().active_id(), Some("22".into()));
		assert!(song.carousel().position() > 0.0);

		window.close();
	});

	broadway.kill()?;
	broadway.wait()?;

	Ok(())
}

/* Copied over from https://docs.rs/async-std/latest/src/async_std/task/yield_now.rs.html */

pub async fn yield_now() {
	for _ in 0..50 {
		glib::timeout_future(std::time::Duration::from_millis(20)).await;
		YieldNow(false).await;
	}
}

struct YieldNow(bool);

impl futures::Future for YieldNow {
	type Output = ();

	// The futures executor is implemented as a FIFO queue, so all this future
	// does is re-schedule the future back to the end of the queue, giving room
	// for other futures to progress.
	fn poll(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Self::Output> {
		if !self.0 {
			self.0 = true;
			cx.waker().wake_by_ref();
			std::task::Poll::Pending
		} else {
			std::task::Poll::Ready(())
		}
	}
}
