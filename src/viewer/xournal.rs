//! Integration with Xournal++ for annotations
//!
//! Automatically launches Xournal++ to annotate the score. On save, the changes
//! are incorporated back into our format.

use super::*;
use anyhow::Context;
use gtk::glib;
use std::{io::Write, process::Command};

pub async fn run_editor(song: &collection::SongFile, page: usize) -> anyhow::Result<()> {
	// TODO don't hardcode here
	let xdg = xdg::BaseDirectories::with_prefix("dinoscore");

	catch!({
		log::debug!("Checking Xournal++ availability");
		let version = Command::new("xournalpp").arg("--version").output()?;
		anyhow::ensure!(version.status.success());
		Ok(())
	})
	.context("Failed to check Xournal++ availability")?;

	let annotations_file = xdg.place_data_file(format!("annotations/{}.xopp", song.uuid()))?;
	let annotations_background_file = annotations_file.parent().unwrap().join({
		let mut name = annotations_file.file_name().unwrap().to_owned();
		name.push(".background.pdf");
		name
	});
	let annotations_export = xdg.place_data_file(format!("annotations/{}.pdf", song.uuid()))?;

	let background_pdf: Vec<u8> = async {
		log::debug!("Creating the PDF background for the file");
		let loader = song.load_pages(|_index, file, data| Ok((data, file.ends_with(".pdf"))));
		let background_pdf = blocking::unblock(move || {
			image_util::concat_files(loader().context("Failed to load pages")?.into())
		})
		.await
		.context("Internal error")?;
		std::fs::write(&annotations_background_file, &background_pdf)
			.context("Failed to write file")?;
		anyhow::Result::<_>::Ok(background_pdf)
	}
	.await
	.context("Failed to create the background PDF for the Xournal document")?;

	if !annotations_file.exists() {
		log::debug!("Creating an empty file for editing");

		catch!({
			let xopp = std::fs::File::create(&annotations_file)?;
			let mut xopp = flate2::write::GzEncoder::new(xopp, Default::default());
			let pdf =
				poppler::Document::from_bytes(&glib::Bytes::from_owned(background_pdf), None)?;

			writeln!(
				xopp,
				r#"<?xml version="1.0" standalone="no"?>
<xournal creator="piegames" fileversion="4">
<title>Xournal++ document - see https://github.com/xournalpp/xournalpp</title>
"#
			)?;

			for index in 0..pdf.n_pages() {
				let page = pdf.page(index).unwrap();
				let (width, height) = page.size();
				writeln!(xopp, r#"<page width="{}" height="{}">"#, width, height)?;
				if index == 0 {
					writeln!(
						xopp,
						r#"<background type="pdf" pageno="{}ll" domain="attach" filename="background.pdf" />"#,
						(index + 1)
					)?;
				} else {
					writeln!(
						xopp,
						r#"<background type="pdf" pageno="{}ll" />"#,
						(index + 1)
					)?;
				}
				writeln!(xopp, "<layer />")?;
				writeln!(xopp, "</page>")?;
			}

			writeln!(xopp, "</xournal>")?;
			anyhow::Result::<_>::Ok(())
		})
		.context("Failed to create Xournal file")?;
	} else {
		std::mem::drop(background_pdf);
	}
	anyhow::ensure!(
		annotations_file.is_file(),
		"'{}' must be a regular file. Please delete whatever is there",
		annotations_file.display()
	);

	blocking::unblock(move || {
		log::debug!("Launching Xournal++ editor (page {})", page);
		let run = Command::new("xournalpp")
			.args(&[
				"--page".as_ref(),
				page.to_string().as_ref(),
				annotations_file.as_os_str(),
			])
			.status()
			.context("Failed to launch Xournal++")?;
		anyhow::ensure!(run.success(), "Xournal++ exited with non-zero status");

		log::debug!("Integrating back the annotations into DiNoScore");
		let run = Command::new("xournalpp")
			.args(&[
				"--export-no-background".as_ref(),
				"--create-pdf".as_ref(),
				annotations_export.as_os_str(),
				annotations_file.as_os_str(),
			])
			.status()
			.context("Failed to launch Xournal++ for export")?;
		anyhow::ensure!(
			run.success(),
			"Xournal++ export exited with non-zero status"
		);

		catch!({
			std::fs::remove_file(annotations_background_file)?;
			anyhow::Result::<_>::Ok(())
		})
		.context("Post-editor cleanup failed")?;

		Ok(())
	})
	.await
}
