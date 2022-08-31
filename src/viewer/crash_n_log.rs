//! Logging and crash handling
//!
//! Panic handler, crash dialogs, log configuration, etc.

use crate::*;
use std::panic::PanicInfo;

type PanicHook = Box<dyn Fn(&PanicInfo<'_>) + 'static + Sync + Send>;

/** Initialize log and panic handling */
pub fn init() -> anyhow::Result<()> {
	let mut logger = fern::Dispatch::new()
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
		.level_for("serde_xml_rs", log::LevelFilter::Warn);

	logger = logger.chain(fern::logger::stdout());
	match catch!({
		// TODO don't hardcode here
		let xdg = xdg::BaseDirectories::with_prefix("dinoscore")?;
		let mut path = xdg.get_state_file("logs");
		std::fs::create_dir_all(&path)?;

		/* Log rotation. Ignore most errors that may occur. */
		for entry in std::fs::read_dir(&path)? {
			let _ = catch!({
				let entry = entry?;
				let modified = entry.metadata()?.modified()?;
				if modified.elapsed().unwrap_or_default()
					> std::time::Duration::from_secs(3600 * 24 * 7)
				{
					std::fs::remove_file(entry.path())?;
				}
				anyhow::Result::<_>::Ok(())
			});
		}

		#[cfg(unix)]
		{
			path.push(
				chrono::Local::now()
					.format("%Y-%m-%d %H:%M.log")
					.to_string(),
			);
		}
		#[cfg(windows)]
		{
			path.push(
				chrono::Local::now()
					.format("%Y-%m-%d %H-%M.log")
					.to_string(),
			);
		}
		let log_file = fern::logger::file(path.clone())?;
		// anyhow::Result::<_>::Ok(fern::DateBased::new(path, "%Y-%m-%d %H:%M.log"))
		anyhow::Result::<_>::Ok((path, log_file))
	})
	.context("Failed to initialize a file for logging")
	{
		Ok((path, log_file)) => {
			logger = logger.chain(log_file);
			/* Initialize logger */
			logger.apply().context("Failed to initialize logger")?;
			log::debug!("Logging to {}", path.display());
		},
		Err(error) => {
			/* Initialize logger and log that we failed to do some file logging */
			logger.apply().context("Failed to initialize logger")?;
			log::warn!("{}", error);
		},
	};

	glib::log_set_writer_func(|level, fields| {
		let level = match level {
			glib::LogLevel::Error | glib::LogLevel::Critical => log::Level::Error,
			glib::LogLevel::Warning => log::Level::Warn,
			glib::LogLevel::Message | glib::LogLevel::Info => log::Level::Info,
			glib::LogLevel::Debug => log::Level::Debug,
		};

		let message = fields
			.iter()
			.find(|field| field.key() == "MESSAGE")
			.and_then(glib::LogField::value_str)
			.unwrap_or("<no message>");
		let domain = fields
			.iter()
			.find(|field| field.key() == "GLIB_DOMAIN")
			.and_then(glib::LogField::value_str)
			.unwrap_or("<unknown>");

		log::log!(target: domain, level, "{}", message);

		glib::LogWriterOutput::Handled
	});
	/* I think those three are mostly redundant by the above, but just in case */
	glib::log_set_default_handler(glib::rust_log_handler);
	glib::set_print_handler(|print| {
		log::info!("Internal gtk message: {print}");
	});
	glib::set_printerr_handler(|print| {
		log::warn!("Internal gtk message: {print}");
	});

	/* Collect some panic hooks before building a custom one upon them */
	#[allow(unused)]
	let default_hook = std::panic::take_hook();
	log_panics::init();
	let log_panics_hook = std::panic::take_hook();

	std::panic::set_hook(Box::new(move |panic_info| {
		panic_hook(panic_info, &log_panics_hook);
	}));
	Ok(())
}

fn panic_hook(panic_info: &PanicInfo, log_panics_hook: &PanicHook) {
	log::error!("An unrecoverable error occured, shutting down …");

	log_panics_hook(panic_info);

	let crash = match write_crash_message(panic_info) {
		Ok(crash) => {
			log::info!("Crash information written to '{}'.", crash.display());
			log::info!(
				"Please open a bug report at 'https://github.com/DiNoScore/DiNoScore/issues'."
			);
			crash
		},
		Err(err) => {
			log::warn!("Failed to write crash information: {}", err);
			log::logger().flush();
			std::process::exit(110);
		},
	};

	log::logger().flush();

	/* Ignore everything that can go wrong from here */

	if let Ok(exe) = std::env::current_exe() {
		use std::process::Command;

		#[cfg(unix)]
		{
			use std::os::unix::process::CommandExt;
			let _ = Command::new(&exe).args(&[&crash]).exec();
		}
		#[cfg(windows)]
		{
			if let Ok(status) = Command::new(&exe).args(&[&crash]).status() {
				std::process::exit(status.code().unwrap_or_default());
			}
		}
	}

	/* If everything went well, this won't be reached */
	std::process::exit(110);
}

fn write_crash_message(info: &std::panic::PanicInfo) -> anyhow::Result<std::path::PathBuf> {
	use std::io::Write;
	// TODO don't hardcode here
	let xdg = xdg::BaseDirectories::with_prefix("dinoscore")?;
	let report = xdg.place_state_file(format!(
		"crash {}.md",
		chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
	))?;
	log::debug!("Writing crash information to '{}'", report.display());
	let mut out = std::fs::File::create(&report)?;

	writeln!(&mut out, "## Crash information\n")?;

	if let Some(message) = info
		.payload()
		.downcast_ref::<&'static str>()
		.copied()
		.or_else(|| info.payload().downcast_ref::<String>().map(|s| &**s))
	{
		writeln!(
			&mut out,
			"**Thread: '{}' panicked with '{}' at {}**\n",
			std::thread::current().name().unwrap_or("unknown"),
			message,
			match info.location() {
				Some(location) => format!(
					"{}, l.{}:{}",
					location.file(),
					location.line(),
					location.column()
				),
				None => "unknown location".into(),
			}
		)?;
	}

	writeln!(&mut out, "Stack trace:\n")?;
	writeln!(&mut out, "```")?;
	write!(&mut out, "{:?}", backtrace::Backtrace::new())?;
	writeln!(&mut out, "```\n")?;

	writeln!(
		&mut out,
		"- DiNoScore version: `{}`{}",
		git_version::git_version!(fallback = "unknown"),
		if cfg!(debug_assertions) {
			", debug build"
		} else {
			""
		}
	)?;
	writeln!(&mut out, "- Operating system: `{}`", std::env::consts::OS)?;
	writeln!(
		&mut out,
		"- Hardware architecture: `{}`",
		std::env::consts::ARCH
	)?;

	Ok(report)
}

/**
 * Show a crash dialog and exit
 *
 * The zeroth argument will be ignored, the first one will be displayed
 * as the path of the crash log.
 *
 * The application will exit with code 110 (Rust default for "panicked"),
 * but there is also the option for the user to directly re-start DiNoScore.
 */
pub fn show_crash_dialog(args: Vec<std::ffi::OsString>) -> ! {
	use gtk::prelude::*;

	let crash_file = &args[1];

	gtk::init().expect("Failed to initialize GTK");
	let dialog = gtk::MessageDialog::new(
		None::<&gtk::Window>,
		gtk::DialogFlags::MODAL,
		gtk::MessageType::Error,
		gtk::ButtonsType::None,
		"DiNoScore crashed :(",
	);

	// TODO don't hardcode here
	let xdg = xdg::BaseDirectories::with_prefix("dinoscore").unwrap();
	let logs_dir = xdg.get_cache_file("logs");
	dialog.set_secondary_use_markup(true);
	dialog.set_secondary_text(Some(&format!(
		"\
		Crash information has been written to <a href=\"file://{crash_file}\">{crash_file}</a>. \
		Recent logs for more information can be found at <a href=\"file://{logs_dir}\">{logs_dir}</a>. \
		Please open a bug report at <a href=\"https://github.com/DiNoScore/DiNoScore/issues\">\
		https://github.com/DiNoScore/DiNoScore/issues</a>. \
		",
		crash_file = std::path::Path::new(crash_file).display(),
		logs_dir = logs_dir.display(),
	)));
	dialog.add_buttons(&[
		("Close", gtk::ResponseType::Close),
		("Restart DiNoScore", gtk::ResponseType::Ok),
	]);
	dialog.set_default_response(gtk::ResponseType::Ok);
	dialog.present();

	let main_loop = glib::MainLoop::new(None, false);

	#[allow(unused_variables)]
	dialog.connect_response(|dialog, response| match response {
		gtk::ResponseType::Ok => {
			/* Exec back into new DiNoScore process */
			if let Ok(exe) = std::env::current_exe() {
				use std::process::Command;

				#[cfg(unix)]
				{
					use std::os::unix::process::CommandExt;
					let _ = Command::new(&exe).exec();
				}
				#[cfg(windows)]
				{
					dialog.destroy();
					if let Ok(status) = Command::new(&exe).status() {
						std::process::exit(status.code().unwrap_or_default());
					}
				}
			}
			/* If everything went well, this won't be reached. */
			std::process::exit(110);
		},
		_ => {
			std::process::exit(110);
		},
	});

	main_loop.run();

	/* This should actually be unreachable */
	log::error!("Crash dialog did not exit as expected. Please file a bug report");
	std::process::exit(111);
}
