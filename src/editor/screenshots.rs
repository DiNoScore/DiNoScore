//! Automatically take some screenshots of the software, disguised as tests

use super::*;
use std::env;

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

	env::set_var("WLR_BACKENDS", "headless");
	let mut sway = std::process::Command::new("sway")
		.args(["-c", "/dev/null", "--verbose"])
		.spawn()
		.context("Failed to start sway")?;
	std::thread::sleep(std::time::Duration::from_secs(1));
	env::set_var("WAYLAND_DISPLAY", "wayland-1");

	pipeline::pipe! {
		gvdb_macros::include_gresource_from_xml!("res/editor/resources.gresource.xml")
		=> glib::Bytes::from_static
		=> &gio::Resource::from_data(&_)?
		=> gio::resources_register
	};
	/* Vendor icons */
	gio::resources_register_include!("icons.gresource").context("Failed to register resources.")?;

	let application = gtk::Application::builder()
		.application_id("de.piegames.dinoscore.editor")
		.flags(gio::ApplicationFlags::NON_UNIQUE)
		.resource_base_path("/de/piegames/dinoscore")
		.build();

	application.connect_startup(gtk_init);

	let runner = |window: EditorWindow| async move {
		/* Set things up */
		let theme = adw::StyleManager::default();
		theme.set_color_scheme(adw::ColorScheme::ForceLight);
		window.present();
		window.queue_draw();
		window.fullscreen();

		/* First of all, load our song */
		let song = SongFile::new(
			"./test/dinoscore/songs/Chopin, Frédéric – Waltzes, Op.64.zip",
			&mut Default::default(),
		)
		.unwrap();
		let load_sheets = song.load_sheets();
		let sheets = blocking::unblock(move || load_sheets()).await.unwrap();
		window.imp().load(sheets, song.index);
		yield_now().await;

		/* Select the first staff */
		window
			.imp()
			.pages_preview
			.get()
			.select_path(&gtk::TreePath::new_first());
		window.imp().editor.get().select_staff(0);
		yield_now().await;

		take_screenshot("gallery/06-editor.png")
			.context("Failed to take screenshot")
			.unwrap();

		/* Select staff #5 (is a repetition start) */
		window.imp().editor.get().select_staff(4);
		yield_now().await;

		take_screenshot("gallery/07-editor-repetition.png")
			.context("Failed to take screenshot")
			.unwrap();

		/* One more example */
		window.imp().pages_preview.get().unselect_all();
		window.imp().pages_preview.get().select_path(&{
			let mut p = gtk::TreePath::new_first();
			p.next();
			p
		});
		window.imp().editor.get().select_staff(2);
		yield_now().await;

		take_screenshot("gallery/08-editor-repetition.png")
			.context("Failed to take screenshot")
			.unwrap();

		window.close();
	};

	application.connect_activate(move |application| {
		let window = EditorWindow::new(application);
		glib::MainContext::default().spawn_local_with_priority(glib::PRIORITY_LOW, runner(window));
	});

	application.run_with_args(&[] as &[&str]);

	sway.kill()?;
	sway.wait()?;

	Ok(())
}

/* Copied over from https://docs.rs/async-std/latest/src/async_std/task/yield_now.rs.html */

pub async fn yield_now() {
	for _ in 0..50 {
		std::thread::sleep(std::time::Duration::from_millis(20));
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
