//! Everything we need to deal with images.
//!
//! Contains helper functions for PDF <-> Pixbuf conversion, and a [`PageImageExt`] trait that
//! abstracts over them in the case we don't care (most of the time, in fact).

use anyhow::Context;
use gdk::prelude::*;

pub trait PageImage {
	fn render(&self, context: &cairo::Context);

	fn render_to_thumbnail(&self, width: i32) -> gdk_pixbuf::Pixbuf;

	fn get_width(&self) -> f64;

	fn get_height(&self) -> f64;
}

impl PageImage for gdk_pixbuf::Pixbuf {
	fn render(&self, context: &cairo::Context) {
		context.set_source_pixbuf(self, 0.0, 0.0);
		context.paint();
	}

	fn render_to_thumbnail(&self, width: i32) -> gdk_pixbuf::Pixbuf {
		self.scale_simple(
			width,
			width * self.get_height() / self.get_width(),
			gdk_pixbuf::InterpType::Bilinear,
		)
		.unwrap()
	}

	fn get_width(&self) -> f64 {
		self.get_width() as f64
	}

	fn get_height(&self) -> f64 {
		self.get_height() as f64
	}
}

pub type PageImageBox = Box<dyn PageImage>;

impl PageImage for poppler::PopplerPage {
	fn render(&self, context: &cairo::Context) {
		self.render(context);
	}

	fn render_to_thumbnail(&self, width: i32) -> gdk_pixbuf::Pixbuf {
		pdf_to_pixbuf(&self, width)
	}

	fn get_width(&self) -> f64 {
		self.get_size().0
	}

	fn get_height(&self) -> f64 {
		self.get_size().1
	}
}

/// A loaded image, together with the raw bytes to save it losslessly
pub enum RawPageImage {
	Raster {
		image: gdk_pixbuf::Pixbuf,
		raw: Vec<u8>,
		/// File name extension; the format of the bytes
		extension: String,
	},
	Vector {
		page: poppler::PopplerPage,
		raw: Vec<u8>,
	},
}

impl RawPageImage {
	pub fn extension(&self) -> &str {
		match self {
			Self::Raster { extension, .. } => &extension,
			Self::Vector { .. } => "pdf",
		}
	}

	pub fn raw(&self) -> &[u8] {
		match self {
			Self::Raster { raw, .. } | Self::Vector { raw, .. } => &raw,
		}
	}
}

impl PageImage for RawPageImage {
	fn render(&self, context: &cairo::Context) {
		(&self).render(context);
	}

	fn render_to_thumbnail(&self, width: i32) -> gdk_pixbuf::Pixbuf {
		(&self).render_to_thumbnail(width)
	}

	fn get_width(&self) -> f64 {
		(&self).get_width()
	}

	fn get_height(&self) -> f64 {
		(&self).get_height()
	}
}

impl PageImage for &RawPageImage {
	fn render(&self, context: &cairo::Context) {
		match self {
			RawPageImage::Vector { page, .. } => page.render(context),
			RawPageImage::Raster { image, .. } => image.render(context),
		}
	}

	fn render_to_thumbnail(&self, width: i32) -> gdk_pixbuf::Pixbuf {
		match self {
			RawPageImage::Vector { page, .. } => page.render_to_thumbnail(width),
			RawPageImage::Raster { image, .. } => image.render_to_thumbnail(width),
		}
	}

	fn get_width(&self) -> f64 {
		match self {
			RawPageImage::Vector { page, .. } => page.get_width(),
			RawPageImage::Raster { image, .. } => image.get_width() as f64,
		}
	}

	fn get_height(&self) -> f64 {
		match self {
			RawPageImage::Vector { page, .. } => page.get_height(),
			RawPageImage::Raster { image, .. } => image.get_height() as f64,
		}
	}
}

/// Split a PDF file into its own pages
// TODO replace with inline_python! once that compiles on stable.
// This will result in better and shorter code
pub fn explode_pdf(pdf: &[u8]) -> anyhow::Result<Vec<Vec<u8>>> {
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

pub fn explode_pdf_full<T>(
	pdf: &[u8],
	mapper: impl Fn(Vec<u8>, poppler::PopplerPage) -> T,
) -> anyhow::Result<Vec<T>> {
	explode_pdf(pdf)
		.context("Failed to split PDF into its pages")?
		.into_iter()
		.map(|bytes| {
			let document = poppler::PopplerDocument::new_from_bytes(
				glib::Bytes::from_owned(bytes.clone()),
				"",
			)?;
			/* This is a guarantee from our explode_pdf function */
			assert!(document.get_n_pages() == 1);
			Ok(mapper(bytes, document.get_page(0).unwrap()))
		})
		.collect::<anyhow::Result<_>>()
		.context("Failed to split legacy PDF into its pages")
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
					file
				} else {
					let image = gdk_pixbuf::Pixbuf::from_stream(
						&gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(file)),
						Option::<&gio::Cancellable>::None,
					)
					.unwrap();
					pixbuf_to_pdf_raw(&image)
				}
			})
			.collect(),
	)
}

/// Create a PDF Document with a single page that wraps a raster image
#[allow(clippy::box_vec)]
pub fn pixbuf_to_pdf_raw(image: &gdk_pixbuf::Pixbuf) -> Vec<u8> {
	let pdf_binary: Vec<u8> = Vec::new();
	let surface = cairo::PdfSurface::for_stream(
		image.get_width() as f64,
		image.get_height() as f64,
		pdf_binary,
	)
	.unwrap();

	let context = cairo::Context::new(&surface);
	context.set_source_pixbuf(image, 0.0, 0.0);
	context.paint();

	surface.flush();

	*surface
		.finish_output_stream()
		.unwrap()
		.downcast::<Vec<u8>>()
		.unwrap()
}

/// Create a PDF Document with a single page that wraps a raster image
pub fn pixbuf_to_pdf(image: &gdk_pixbuf::Pixbuf) -> poppler::PopplerDocument {
	poppler::PopplerDocument::new_from_bytes(glib::Bytes::from_owned(pixbuf_to_pdf_raw(image)), "")
		.unwrap()
}

/// Render a PDF page to a preview image with fixed width
pub fn pdf_to_pixbuf(page: &poppler::PopplerPage, width: i32) -> gdk_pixbuf::Pixbuf {
	let surface = cairo::ImageSurface::create(
		cairo::Format::Rgb24,
		width,
		(width as f64 * page.get_size().1 / page.get_size().0) as i32,
	)
	.unwrap();
	let context = cairo::Context::new(&surface);
	let scale = width as f64 / page.get_size().0;
	context.set_antialias(cairo::Antialias::Best);
	context.scale(scale, scale);
	context.set_source_rgb(1.0, 1.0, 1.0);
	context.paint();
	page.render(&context);
	surface.flush();

	gdk::pixbuf_get_from_surface(&surface, 0, 0, surface.get_width(), surface.get_height()).unwrap()
}
