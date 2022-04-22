//! Application structure
//!
//! - [`window`]: Application window. Also does fullscreen handling
//!   - [`library_widget`]: The library/song selection pane
//!   - [`song_widget`]: The "play song" pane
//!     - [`song_page`]: A single page on the song carousel

#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::tabs_in_doc_comments)]

use anyhow::Context;
use dinoscore::{prelude::*, *};

mod crash_n_log;
mod library_widget;
mod pedal;
mod song_page;
mod song_widget;
mod window;
mod xournal;

fn gtk_init(_application: &gtk::Application) {
	let _ = gio::ThemedIcon::static_type();
	let _ = library_widget::LibraryWidget::static_type();
	let _ = song_widget::SongWidget::static_type();
	let _ = song_page::SongPage::static_type();
	adw::init();
}

fn main() -> anyhow::Result<()> {
	{
		/* If we get called with an argument, show a crash dialog and exit */
		let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
		/* As usual, ignore arg0 */
		if args.len() > 1 {
			crash_n_log::show_crash_dialog(args);
			/* Never returns */
		}
	}

	crash_n_log::init()?;
	log::debug!(
		"DiNoScore version {}.",
		git_version::git_version!(fallback = "unknown")
	);

	gio::resources_register_include!("viewer.gresource")
		.context("Failed to register resources.")?;

	let application = gtk::Application::new(
		Some("de.piegames.dinoscore.viewer"),
		gio::ApplicationFlags::NON_UNIQUE,
	);

	application.connect_startup(gtk_init);

	application.connect_activate(move |application| {
		let window = window::Window::new(application);

		application.set_accels_for_action("window.close", &["<Primary>Q"]);
		application.set_accels_for_action("win.toggle-fullscreen", &["F11"]);

		window.present();
		log::info!("Application started");
	});

	application.run_with_args(&[] as &[&str]);

	log::info!("Thank you for using DiNoScore.");
	log::logger().flush();

	Ok(())
}
