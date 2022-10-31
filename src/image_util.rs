//! Everything we need to deal with images.
//!
//! Contains helper functions for PDF <-> Pixbuf conversion, and a [`PageImageExt`] trait that
//! abstracts over them in the case we don't care (most of the time, in fact).

use anyhow::Context;

use adw::prelude::*;
use gdk::{cairo, gdk_pixbuf};
use gtk::{gdk, gio, glib, glib::clone, prelude::*};
use gtk4 as gtk;
use libadwaita as adw;

/// An image file, in memory but compressed
///
/// It may be either an image or a single-page PDF. This invariant is checked at
/// first load. The image is kept as-is in memory, and it is only decompressed
/// when needed to save RAM.
pub struct PageImage {
	raw: Vec<u8>,
	/// File name extension; the format of the bytes
	extension: String,
	// For raster images: Size in pixels
	// For vector images: Size of the PDF page in *units*
	width: f64,
	height: f64,
}

impl PageImage {
	pub fn from_pdf(raw: Vec<u8>) -> anyhow::Result<Self> {
		let pdf = poppler::Document::from_bytes(&glib::Bytes::from(&raw), None)
			.context("Failed to load PDF")?;
		anyhow::ensure!(pdf.n_pages() == 1, "PDF file must have exactly one page");
		let page = pdf.page(0).unwrap();
		Ok(Self::from_pdf_page(raw, &page))
	}

	/// Only used by the legacy API
	pub fn from_pdf_page(raw: Vec<u8>, page: &poppler::Page) -> Self {
		Self {
			raw,
			extension: "pdf".into(),
			width: page.size().0,
			height: page.size().1,
		}
	}

	pub fn from_image(raw: Vec<u8>, extension: String) -> anyhow::Result<Self> {
		let pixbuf = gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(raw.clone()))
			.context("Failed to load image")?;
		Ok(Self {
			raw,
			extension,
			width: pixbuf.width() as f64,
			height: pixbuf.height() as f64,
		})
	}

	pub fn is_pdf(&self) -> bool {
		&self.extension == "pdf"
	}

	pub fn extension(&self) -> &str {
		&self.extension
	}

	pub fn raw(&self) -> &[u8] {
		&self.raw
	}

	/// The width of the coordinate system for this image
	pub fn reference_width(&self) -> f64 {
		self.width
	}

	/// The height of the coordinate system for this image
	pub fn reference_height(&self) -> f64 {
		self.height
	}

	/// The maximum sensible width to render at (None for vector images)
	pub fn max_width(&self) -> Option<f64> {
		(!self.is_pdf()).then(|| self.width)
	}

	/// Load and render this image to a pixbuf.
	///
	/// The result will have at most the requested width and be scaled with
	/// preserved aspect ratio. If the source is a raster image, it will never
	/// be scaled up.
	pub fn render_scaled(&self, width: i32) -> gdk_pixbuf::Pixbuf {
		/* We can panic on error here because we are just double-checking a previously-enforced invariant */

		if self.is_pdf() {
			let pdf = poppler::Document::from_bytes(&glib::Bytes::from(&self.raw), None)
				.expect("Failed to load PDF");
			assert!(pdf.n_pages() == 1, "PDF file must have exactly one page");
			let page = pdf.page(0).unwrap();
			pdf_to_pixbuf(&page, width).expect("Failed to render PDF")
		} else {
			let pixbuf = gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(self.raw.clone()))
				.expect("Failed to load image");
			if width as f64 >= self.width {
				pixbuf
			} else {
				pixbuf
					.scale_simple(
						width,
						(width as f64 * self.height / self.width).ceil() as i32,
						gdk_pixbuf::InterpType::Bilinear,
					)
					.expect("Failed to scale image")
			}
		}
	}

	/// Load and render this image to a [cairo::Context].
	pub fn render_cairo(&self, context: &cairo::Context) -> cairo::Result<()> {
		/* We can panic on error here because we are just double-checking a previously-enforced invariant */

		if self.is_pdf() {
			let pdf = poppler::Document::from_bytes(&glib::Bytes::from(&self.raw), None)
				.expect("Failed to load PDF");
			assert!(pdf.n_pages() == 1, "PDF file must have exactly one page");
			let page = pdf.page(0).unwrap();
			page.render(&context);
			context.status()
		} else {
			let pixbuf = gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(self.raw.clone()))
				.expect("Failed to load image");
			context.set_source_pixbuf(&pixbuf, 0.0, 0.0);
			context.paint()
		}
	}

	/// If this is a PDF embedding an image, try to extract it
	///
	/// Panics if `self` is not PDF based
	pub fn extract_image(&self) -> anyhow::Result<Self> {
		assert!(self.is_pdf());

		let (extraction, pdf_n_pages) =
			extract_pdf_images_raw(&self.raw).context("Failed to extract images from PDF")?;
		assert_eq!(pdf_n_pages, 1); /* Double-check a previously-enforced invariant */

		anyhow::ensure!(extraction.len() > 0, "Did not find any images to extract");
		anyhow::ensure!(
			extraction.len() == 1,
			"Extraction produced more than one image per page"
		);

		let (extension, raw) = extraction.into_iter().next().unwrap();
		Self::from_image(raw, extension)
	}
}

/// Split a PDF file into its own pages
pub fn explode_pdf_raw(pdf: &[u8]) -> anyhow::Result<Vec<Vec<u8>>> {
	use pyo3::{conversion::IntoPy, types::IntoPyDict};
	let gil = pyo3::Python::acquire_gil();
	let py = gil.python();

	let locals = [("pdf", pdf.into_py(py))].into_py_dict(py);
	py.run(
		r#"
from pikepdf import Pdf
from io import BytesIO

pdf = Pdf.open(BytesIO(bytes(pdf)))

pages = []
for page in pdf.pages:
	buf = BytesIO(bytearray())
	dst = Pdf.new()
	dst.pages.append(page)
	dst.save(buf)
	del dst
	pages += [buf.getvalue()]
"#,
		None,
		Some(locals),
	)?;

	Ok(locals.get_item("pages").unwrap().extract().unwrap())
}

/// Split a PDF file into its own pages, map the result to something sensible
pub fn explode_pdf(
	pdf: &[u8],
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<(Vec<u8>, poppler::Page)>>> {
	Ok(explode_pdf_raw(pdf)
		.context("Failed to split PDF into its pages")?
		.into_iter()
		.map(|bytes| {
			let document =
				poppler::Document::from_bytes(&glib::Bytes::from_owned(bytes.clone()), None)
					.context("Failed to split legacy PDF into its pages")?;
			/* This is a guarantee from our explode_pdf function */
			assert!(document.n_pages() == 1);
			Ok((bytes, document.page(0).unwrap()))
		}))
}

/// Extract all raster images from a PDF
///
/// Return type: `([(format, bytes)], pdf_n_pages)`
pub fn extract_pdf_images_raw(pdf: &[u8]) -> anyhow::Result<(Vec<(String, Vec<u8>)>, usize)> {
	use pyo3::{conversion::IntoPy, types::IntoPyDict};
	let gil = pyo3::Python::acquire_gil();
	let py = gil.python();

	let locals = [("pdf", pdf.into_py(py))].into_py_dict(py);
	py.run(
		r#"
import pikepdf
from pikepdf import Pdf, PdfImage
from io import BytesIO

pdf = Pdf.open(BytesIO(bytes(pdf)))
n_pages = len(pdf.pages)

images = []

for page in pdf.pages:
	for image in list(page.images.values()):
		# Horrible hack: https://github.com/pikepdf/pikepdf/issues/269
		# (This is likely not a bug in PikePDF, but just the situation generally
		# being massively fucked up)
		if hasattr(image, "DecodeParms"):
			if isinstance(image.DecodeParms, pikepdf.objects.Array):
				for param in image.DecodeParms:
					if hasattr(param, "BlackIs1"):
						param.BlackIs1 = False
			else:
				if hasattr(image.DecodeParms, "BlackIs1"):
					image.DecodeParms.BlackIs1 = False

		buf = BytesIO(bytearray())
		format = PdfImage(image).extract_to(stream=buf)
		images += [(format[1:], buf.getvalue())]

# If the extractor did not find enough images, try some harder methods
# https://github.com/pikepdf/pikepdf/issues/366
if len(images) < n_pages:
	images = []
	print("[DEBUG] Using custom extractor")
	for object in pdf.objects:
		if isinstance(object, pikepdf.objects.Array):
			continue
		if getattr(object, "Type", None) == "/XObject" and getattr(object, "Subtype", None) == "/Image":
			buf = BytesIO(bytearray())
			format = PdfImage(object).extract_to(stream=buf)
			images += [(format[1:], buf.getvalue())]

# Return type: have the images plus the number of PDF pages in a tuple
images = (images, n_pages)
"#,
		None,
		Some(locals),
	)
	// TODO replace with inspect_err once stable
	.map_err(|err| {
		err.print(py);
		err
	})?;

	Ok(locals.get_item("images").unwrap().extract().unwrap())
}

pub fn concat_pdfs(pdfs: Vec<Vec<u8>>) -> anyhow::Result<Vec<u8>> {
	use pyo3::{conversion::IntoPy, types::IntoPyDict};
	let gil = pyo3::Python::acquire_gil();
	let py = gil.python();

	let locals = [("pdfs", pdfs.into_py(py))].into_py_dict(py);
	py.run(
		r#"
from pikepdf import Pdf
from io import BytesIO

out = Pdf.new()

for pdf in pdfs:
	src = Pdf.open(BytesIO(bytes(pdf)))
	out.pages.extend(src.pages)

buf = BytesIO(bytearray())
out.save(buf)
del out
buf = buf.getvalue()
"#,
		None,
		Some(locals),
	)?;

	Ok(locals.get_item("buf").unwrap().extract().unwrap())
}

pub fn concat_files(pdfs: Vec<(Vec<u8>, bool)>) -> anyhow::Result<Vec<u8>> {
	concat_pdfs(
		pdfs.into_iter()
			.map(|(file, is_pdf): (Vec<u8>, bool)| {
				if is_pdf {
					Ok(file)
				} else {
					let image = gdk_pixbuf::Pixbuf::from_stream(
						&gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(file)),
						Option::<&gio::Cancellable>::None,
					)
					.unwrap();
					pixbuf_to_pdf_raw(&image).context("Failed to embed the image in a PDF")
				}
			})
			.collect::<anyhow::Result<_>>()?,
	)
}

/// Create a PDF Document with a single page that wraps a raster image
pub fn pixbuf_to_pdf_raw(image: &gdk_pixbuf::Pixbuf) -> cairo::Result<Vec<u8>> {
	/* We want our PDF page to have a rather sane page size, and using the pixel size of the image
	 * may not be sane depending on its resolution. So instead, we norm it to the area of a DIN A4
	 * page (≈1/16 m²), while keeping the aspect ratio.
	 *
	 * Of course this is just a heuristic that works best for when the original image is roughly the
	 * same size, but it should still work reasonably well for deviations ×/÷ 2.
	 */
	let image_area = image.width() as f64 * image.height() as f64;
	let target_area = 595.2756 * 841.8898;
	let scale = (target_area / image_area).sqrt();

	let surface = cairo::PdfSurface::for_stream(
		image.width() as f64 * scale,
		image.height() as f64 * scale,
		Vec::new(),
	)
	.unwrap();

	let context = cairo::Context::new(&surface)?;
	context.scale(scale, scale);
	context.set_source_pixbuf(image, 0.0, 0.0);
	context.paint()?;

	surface.flush();

	Ok(*surface
		.finish_output_stream()
		.unwrap()
		.downcast::<Vec<u8>>()
		.unwrap())
}

/// Create a PDF Document with a single page that wraps a raster image
pub fn pixbuf_to_pdf(image: &gdk_pixbuf::Pixbuf) -> cairo::Result<poppler::Document> {
	pipeline::pipe! {
		pixbuf_to_pdf_raw(image)?
		=> glib::Bytes::from_owned
		=> poppler::Document::from_bytes(&_, None).unwrap()
		=> cairo::Result::Ok
	}
}

/// Render a PDF page to a preview image with fixed width
pub fn pdf_to_pixbuf(page: &poppler::Page, width: i32) -> cairo::Result<gdk_pixbuf::Pixbuf> {
	let surface = cairo::ImageSurface::create(
		cairo::Format::Rgb24,
		width,
		(width as f64 * page.size().1 / page.size().0) as i32,
	)
	.unwrap();
	let context = cairo::Context::new(&surface)?;
	let scale = width as f64 / page.size().0;
	context.set_antialias(cairo::Antialias::Best);
	context.scale(scale, scale);
	context.set_source_rgb(1.0, 1.0, 1.0);
	context.paint()?;
	page.render(&context);
	surface.flush();

	Ok(gdk::pixbuf_get_from_surface(&surface, 0, 0, surface.width(), surface.height()).unwrap())
}
