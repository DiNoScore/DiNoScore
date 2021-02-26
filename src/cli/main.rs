
use dinoscore::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
	gtk::init().unwrap();

	use clap::*;
	let command_upgrade = SubCommand::with_name("upgrade")
		.about("Upgrade a list of songs to the newest version")
		.arg(
			Arg::with_name("output")
				.long("out-dir")
				.short("o")
				.takes_value(true)
				.required(true)
				.help("Output directory"),
		)
		.arg(
			Arg::with_name("overwrite")
				.long("overwrite")
				.short("f")
				.help("Overwrite existing files in the output directory"),
		)
		.arg(
			Arg::with_name("input-files")
				.help("Files to upgrade")
				.required(true)
				.multiple(true)
				.index(1),
		)
		.setting(AppSettings::ArgRequiredElseHelp);
	let clap = App::new(crate_name!())
		.version(crate_version!())
		.author(crate_authors!())
		.about("CLI tools for DiNoScore")
		.setting(AppSettings::SubcommandRequiredElseHelp)
		.setting(AppSettings::DisableHelpFlags)
		.setting(AppSettings::VersionlessSubcommands)
		.subcommand(command_upgrade);

	let matches = clap.get_matches();

	if let Some(matches) = matches.subcommand_matches("upgrade") {
		let inputs = matches.values_of_os("input-files").unwrap();
		let overwrite = matches.is_present("overwrite");
		let output_dir = matches.value_of_os("output").unwrap();
		let output_dir = std::path::Path::new(output_dir);

		std::fs::create_dir_all(output_dir)?;

		for input in inputs {
			let input: &std::path::Path = input.as_ref();
			println!("Upgrading '{}'", input.display());
			let output_path = output_dir.join(input.file_name().unwrap());
			let mut song = futures::executor::block_on(collection::SongFile::new(input));
			let pdf = song.load_sheet().into_inner();
			let thumbnail = song.thumbnail().cloned();
			let meta = song.index;
			let iter_pages = || (0..pdf.get_n_pages())
				.map(|i| pdf.get_page(i).unwrap())
				.map(From::from);
			let thumbnail = thumbnail
				.or_else(|| collection::SongFile::generate_thumbnail(&meta, iter_pages()));
			collection::SongFile::save(
				output_path,
				meta,
				iter_pages(),
				thumbnail,
				overwrite,
			);
		}
	}

	Ok(())
}
