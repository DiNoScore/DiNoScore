use gtk4 as gtk;

use anyhow::Context;
use clap::{Parser, Subcommand};
use dinoscore::*;
use std::path::PathBuf;
use typed_index_collections::TiVec;

#[derive(Debug, Subcommand)]
enum CliCommand {
	/// Upgrade a list of songs to the newest version
	#[clap(arg_required_else_help = true)]
	Upgrade {
		/// Output directory
		#[clap(short = 'o', long = "out-dir")]
		output: PathBuf,
		/// Overwrite existing files in the output directory
		#[clap(short = 'f', long)]
		overwrite: bool,
		/// Files to upgrade
		#[clap(min_values = 1)]
		input_files: Vec<PathBuf>,
	},
	/// Helper tool for v3→v4 format migration. If the sheets are represented as PDFs which embed
	/// raster images, try to extract them.
	#[clap(arg_required_else_help = true)]
	V4ExtractImages {
		/// Output directory
		#[clap(short = 'o', long = "out-dir")]
		output: PathBuf,
		/// Overwrite existing files in the output directory
		#[clap(short = 'f', long)]
		overwrite: bool,
		/// Files to upgrade
		#[clap(min_values = 1)]
		input_files: Vec<PathBuf>,
		/// Continue even though the extraction failed (e.g. did not provide
		/// the expected number of images). Does not apply to other failures,
		/// like corrupt data. A warning will be printed.
		#[clap(short, long)]
		ignore_errors: bool,
	},
	/// Run the automatic staff recognition again. It will try its best to
	/// merge its results with the existing staves, so that the parts and
	/// repetition information is preserved.
	#[clap(arg_required_else_help = true)]
	ReRecognize {
		/// Output directory
		#[clap(short = 'o', long = "out-dir")]
		output: PathBuf,
		/// Overwrite existing files in the output directory
		#[clap(short = 'f', long)]
		overwrite: bool,
		/// Files to upgrade
		#[clap(min_values = 1)]
		input_files: Vec<PathBuf>,
		/// Continue even though the extraction failed (e.g. did not provide
		/// the expected number of images). Does not apply to other failures,
		/// like corrupt data. A warning will be printed.
		#[clap(short, long)]
		ignore_errors: bool,
	},
	/// Regenerate the thumbnail image
	#[clap(arg_required_else_help = true)]
	RegenerateThumbnail {
		/// Output directory
		#[clap(short = 'o', long = "out-dir")]
		output: PathBuf,
		/// Overwrite existing files in the output directory
		#[clap(short = 'f', long)]
		overwrite: bool,
		/// Files to upgrade
		#[clap(min_values = 1)]
		input_files: Vec<PathBuf>,
	},
}

#[derive(Debug, Parser)]
#[clap(
	version,
	author,
	about = "CLI tools for DiNoScore",
	arg_required_else_help = true,
	propagate_version = true
)]
struct DinoscoreCli {
	#[clap(subcommand)]
	command: CliCommand,
}

fn main() -> anyhow::Result<()> {
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

	gtk::init().unwrap();

	let app = DinoscoreCli::parse();

	match app.command {
		CliCommand::Upgrade {
			output,
			overwrite,
			input_files,
		} => {
			std::fs::create_dir_all(&output)?;

			for input in input_files {
				log::info!("Upgrading '{}'", input.display());

				let output_path = output.join(input.file_name().unwrap());
				let song = collection::SongFile::new(input, &mut Default::default())
					.context("Corrupt song file")?;
				let sheets: TiVec<_, PageImage> =
					song.load_sheets()().context("Failed to load sheets")?;
				let thumbnail = song.thumbnail().cloned();
				let mut meta = song.index;
				meta.version_uuid = uuid::Uuid::new_v4();
				let thumbnail = thumbnail.or_else(|| {
					collection::SongFile::generate_thumbnail(&meta, &sheets)
						.expect("Failed to generate thumbnail")
				});
				collection::SongFile::save(output_path, meta, &sheets, thumbnail, overwrite)?;
			}
		},
		CliCommand::V4ExtractImages {
			output,
			overwrite,
			input_files,
			ignore_errors,
		} => {
			std::fs::create_dir_all(&output)?;

			for input in input_files {
				log::info!("Extracting '{}'", input.display());

				let output_path = output.join(input.file_name().unwrap());
				let song = collection::SongFile::new(input, &mut Default::default())
					.context("Corrupt song file")?;
				let mut sheets: TiVec<_, PageImage> =
					song.load_sheets()().context("Failed to load sheets")?;
				for (i, sheet) in sheets.iter_mut_enumerated() {
					if !sheet.is_pdf() {
						log::info!("Page {i} is not a PDF; skipping");
						continue;
					}
					match sheet.extract_image() {
						Ok(extracted) => *sheet = extracted,
						Err(e) => {
							if ignore_errors {
								log::warn!("Failed to extract page {i}: {e}");
								continue;
							} else {
								log::error!("Failed to extract page {i}: {e}");
								return Err(e);
							}
						},
					}
				}
				let thumbnail = song.thumbnail().cloned();
				let mut meta = song.index;
				meta.version_uuid = uuid::Uuid::new_v4();
				collection::SongFile::save(output_path, meta, &sheets, thumbnail, overwrite)?;
			}
		},
		CliCommand::ReRecognize {
			output,
			overwrite,
			input_files,
			ignore_errors,
		} => {
			std::fs::create_dir_all(&output)?;

			let mut troubled_files = std::collections::BTreeSet::new();
			for input in input_files {
				log::info!("Updating '{}'", input.display());

				let output_path = output.join(input.file_name().unwrap());
				let song = collection::SongFile::new(&input, &mut Default::default())
					.context("Corrupt song file")?;
				let mut sheets: TiVec<_, PageImage> =
					song.load_sheets()().context("Failed to load sheets")?;
				let thumbnail = song.thumbnail().cloned();
				let mut meta = song.index;

				for (page, sheet) in sheets.iter_mut_enumerated() {
					log::info!("Detecting page {page}");
					let image = sheet.render_scaled(400);
					let detected_staves: Vec<collection::Staff> =
						recognition::recognize_staves(&image, page);

					let staves = meta
						.staves
						.iter_mut()
						.filter(|staff| staff.page() == page)
						.collect::<Vec<_>>();

					/* Blank pages should not be included in the document, but actually it may happen */
					if staves.is_empty() {
						continue;
					}

					let result = catch!({
						anyhow::ensure!(detected_staves.len() == staves.len(),
							"Detection did not give the expected number of staves: Expected {} but detected {}", staves.len(), detected_staves.len());

						/* Update with our new results */
						staves
							.into_iter()
							.zip(detected_staves)
							.for_each(|(old, new)| *old = new);

						anyhow::Result::Ok(())
					});
					if let Err(e) = result {
						if ignore_errors {
							log::warn!("Failed to autodetect page {page}: {e}");
							troubled_files.insert(input.display().to_string());
							continue;
						} else {
							log::error!("Failed to autodetect page {page}: {e}");
							return Err(e);
						}
					}
				}
				meta.version_uuid = uuid::Uuid::new_v4();
				collection::SongFile::save(output_path, meta, &sheets, thumbnail, overwrite)?;
			}

			log::info!("Done.");
			if !troubled_files.is_empty() {
				log::warn!(
					"The following files had issues and should be inspected manually: {:?}",
					troubled_files
				);
			}
		},
		CliCommand::RegenerateThumbnail {
			output,
			overwrite,
			input_files,
		} => {
			std::fs::create_dir_all(&output)?;

			for input in input_files {
				log::info!("Regenerating '{}'", input.display());

				let output_path = output.join(input.file_name().unwrap());
				let song = collection::SongFile::new(input, &mut Default::default())
					.context("Corrupt song file")?;
				let sheets: TiVec<_, PageImage> =
					song.load_sheets()().context("Failed to load sheets")?;
				let mut meta = song.index;
				meta.version_uuid = uuid::Uuid::new_v4();
				let thumbnail = collection::SongFile::generate_thumbnail(&meta, &sheets)
					.expect("Failed to generate thumbnail");
				collection::SongFile::save(output_path, meta, &sheets, thumbnail, overwrite)?;
			}
		},
	}

	Ok(())
}
