//! Everything we need to deal with images.
//!
//! Wraps PDF rendering (via poppler) and raster image loading (via glycin)
//! behind the [`PageImage`] type so callers don't need to care which they have.

use anyhow::Context;

use adw::prelude::*;
use gdk::cairo;
use gtk::{gdk, gio, glib, glib::clone, prelude::*};
use gtk4 as gtk;
use libadwaita as adw;

pub fn load_image_frame(raw: &[u8]) -> anyhow::Result<glycin::Frame> {
	use glycin::MemoryFormatSelection as Sel;
	// Use futures_lite's block_on, which has different reentrancy semantics than futures'
	// TODO make this entire function async to make this less ugly
	futures_lite::future::block_on(async {
		let mut loader = glycin::Loader::new_bytes(glib::Bytes::from(raw));
		loader
			// Tell glycin to use its own thread
			.main_context_selector(glycin::MainContextSelector::Managed)
			.sandbox_selector(glycin::SandboxSelector::NotSandboxed)
			.accepted_memory_formats(Sel::G8 | Sel::G8a8 | Sel::R8g8b8 | Sel::R8g8b8a8);
		loader.load().await?.next_frame().await
	})
	.map_err(Into::into)
}

pub fn load_image_texture(raw: &[u8]) -> anyhow::Result<gdk::Texture> {
	Ok(load_image_frame(raw)?.texture())
}

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
		let frame = load_image_frame(&raw).context("Failed to load image")?;
		Ok(Self {
			raw,
			extension,
			width: frame.width() as f64,
			height: frame.height() as f64,
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

	/// Load and render this image to a texture.
	///
	/// The result will have at most the requested width and be scaled with
	/// preserved aspect ratio. If the source is a raster image, it will never
	/// be scaled up.
	pub fn render_scaled(&self, width: i32) -> gdk::Texture {
		/* We can panic on error here because we are just double-checking a previously-enforced invariant */

		if self.is_pdf() {
			let pdf = poppler::Document::from_bytes(&glib::Bytes::from(&self.raw), None)
				.expect("Failed to load PDF");
			assert!(pdf.n_pages() == 1, "PDF file must have exactly one page");
			let page = pdf.page(0).unwrap();
			pdf_to_texture(&page, width).expect("Failed to render PDF")
		} else {
			let frame = load_image_frame(&self.raw).expect("Failed to load image");
			if width as f64 >= self.width {
				frame.texture()
			} else {
				scale_frame(&frame, width)
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
			let texture = load_image_texture(&self.raw).expect("Failed to load image");
			context.set_source_surface(&texture_to_surface(&texture), 0.0, 0.0)?;
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

/// The routines below drive pikepdf through an embedded Python interpreter,
/// which we only have on unix (see the `pyo3` dependency in Cargo.toml).
#[cfg(not(unix))]
fn no_python() -> anyhow::Error {
	anyhow::anyhow!("This build has no Python support, which is required to manipulate PDF files")
}

/// Split a PDF file into its own pages
#[cfg(not(unix))]
pub fn explode_pdf_raw(_pdf: &[u8]) -> anyhow::Result<Vec<Vec<u8>>> {
	Err(no_python())
}

/// Split a PDF file into its own pages
#[cfg(unix)]
pub fn explode_pdf_raw(pdf: &[u8]) -> anyhow::Result<Vec<Vec<u8>>> {
	use pyo3::{conversion::IntoPy, types::IntoPyDict};
	pyo3::Python::with_gil(|py| {
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
	})
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
#[cfg(not(unix))]
pub fn extract_pdf_images_raw(_pdf: &[u8]) -> anyhow::Result<(Vec<(String, Vec<u8>)>, usize)> {
	Err(no_python())
}

/// Extract all raster images from a PDF
///
/// Return type: `([(format, bytes)], pdf_n_pages)`
#[cfg(unix)]
pub fn extract_pdf_images_raw(pdf: &[u8]) -> anyhow::Result<(Vec<(String, Vec<u8>)>, usize)> {
	use pyo3::{conversion::IntoPy, types::IntoPyDict};
	pyo3::Python::with_gil(|py| {
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
	})
}

#[cfg(not(unix))]
pub fn concat_pdfs(_pdfs: Vec<Vec<u8>>) -> anyhow::Result<Vec<u8>> {
	Err(no_python())
}

#[cfg(unix)]
pub fn concat_pdfs(pdfs: Vec<Vec<u8>>) -> anyhow::Result<Vec<u8>> {
	use pyo3::{conversion::IntoPy, types::IntoPyDict};
	pyo3::Python::with_gil(|py| {
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
	})
}

pub fn concat_files(pdfs: Vec<(Vec<u8>, bool)>) -> anyhow::Result<Vec<u8>> {
	concat_pdfs(
		pdfs.into_iter()
			.map(|(file, is_pdf): (Vec<u8>, bool)| {
				if is_pdf {
					Ok(file)
				} else {
					let image = load_image_texture(&file).expect("Failed to load image");
					image_to_pdf_raw(&image).context("Failed to embed the image in a PDF")
				}
			})
			.collect::<anyhow::Result<_>>()?,
	)
}

/// Create a PDF Document with a single page that wraps a raster image
pub fn image_to_pdf_raw(image: &gdk::Texture) -> cairo::Result<Vec<u8>> {
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
	context.set_source_surface(texture_to_surface(image), 0.0, 0.0)?;
	context.paint()?;

	surface.flush();

	Ok(*surface
		.finish_output_stream()
		.unwrap()
		.downcast::<Vec<u8>>()
		.unwrap())
}

/// Create a PDF Document with a single page that wraps a raster image
pub fn image_to_pdf(image: &gdk::Texture) -> cairo::Result<poppler::Document> {
	pipeline::pipe! {
		image_to_pdf_raw(image)?
		=> glib::Bytes::from_owned
		=> poppler::Document::from_bytes(&_, None).unwrap()
		=> cairo::Result::Ok
	}
}

/// Render a PDF page to a preview image with fixed width
pub fn pdf_to_texture(page: &poppler::Page, width: i32) -> cairo::Result<gdk::Texture> {
	let mut surface = cairo::ImageSurface::create(
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
	std::mem::drop(context);

	let bytes = glib::Bytes::from(&*surface.data().unwrap());
	let texture = gdk::MemoryTexture::new(
		surface.width(),
		surface.height(),
		if cfg!(target_endian = "big") {
			gdk::MemoryFormat::X8r8g8b8
		} else {
			gdk::MemoryFormat::B8g8r8x8
		},
		&bytes,
		surface.stride() as usize,
	);
	Ok(texture.upcast())
}

pub fn scale_frame(frame: &glycin::Frame, target_width: i32) -> gdk::Texture {
	use fast_image_resize as fr;
	use glycin::MemoryFormat as GF;

	let (gdk_format, pixel_type, bpp) = match frame.memory_format() {
		GF::G8 => (gdk::MemoryFormat::G8, fr::PixelType::U8, 1),
		GF::G8a8 => (gdk::MemoryFormat::G8a8, fr::PixelType::U8x2, 2),
		GF::R8g8b8 => (gdk::MemoryFormat::R8g8b8, fr::PixelType::U8x3, 3),
		GF::R8g8b8a8 => (gdk::MemoryFormat::R8g8b8a8, fr::PixelType::U8x4, 4),
		other => unreachable!("glycin returned an unrequested format: {other:?}"),
	};

	let (src_w, src_h) = (frame.width(), frame.height());
	let dst_w = target_width as u32;
	let dst_h = (dst_w as f64 * src_h as f64 / src_w as f64).ceil() as u32;

	// fast_image_resize wants tightly-packed input
	let row_bytes = src_w as usize * bpp;
	let stride = frame.stride() as usize;
	let src_bytes: Vec<u8> = if stride == row_bytes {
		frame.buf_slice().to_vec()
	} else {
		(0..src_h as usize)
			.flat_map(|y| {
				frame.buf_slice()[y * stride..y * stride + row_bytes]
					.iter()
					.copied()
			})
			.collect()
	};

	let src = fr::images::Image::from_vec_u8(src_w, src_h, src_bytes, pixel_type)
		.expect("buffer size doesn't match frame dimensions");
	let mut dst = fr::images::Image::new(dst_w, dst_h, pixel_type);
	let options =
		fr::ResizeOptions::new().resize_alg(fr::ResizeAlg::Convolution(fr::FilterType::Hamming));
	fr::Resizer::new()
		.resize(&src, &mut dst, &options)
		.expect("resize failed");

	let bytes = glib::Bytes::from(dst.buffer());
	gdk::MemoryTexture::new(
		dst_w as i32,
		dst_h as i32,
		gdk_format,
		&bytes,
		dst_w as usize * bpp,
	)
	.upcast()
}

/// Convert a GDK Texture to a Cairo ImageSurface
pub fn texture_to_surface(texture: &gdk::Texture) -> cairo::ImageSurface {
	let mut surface =
		cairo::ImageSurface::create(cairo::Format::ARgb32, texture.width(), texture.height())
			.unwrap();
	let stride = surface.stride() as usize;
	{
		let mut data = surface.data().unwrap();
		texture.download(&mut data, stride);
	}
	surface
}

#[cfg(test)]
mod test {
	use super::*;

	#[test]
	fn test_render_pdf_thumbnail() {
		let raw = std::fs::read("./test/image_util/pdf_page.pdf").unwrap();
		let page = PageImage::from_pdf(raw).unwrap();
		let thumbnail = page.render_scaled(400);

		// thumbnail.save_to_png("./test/image_util/pdf_page_thumbnail.png").unwrap();

		let expected = std::fs::read("./test/image_util/pdf_page_thumbnail.png").unwrap();
		let actual = thumbnail.save_to_png_bytes();
		assert_eq!(
			&*actual, &expected,
			"PDF thumbnail does not match golden master"
		);
	}

	#[test]
	fn test_render_raster_thumbnail() {
		let raw = std::fs::read("./test/image_util/raster_page.tif").unwrap();
		let page = PageImage::from_image(raw, "tif".into()).unwrap();
		let thumbnail = page.render_scaled(400);

		// thumbnail.save_to_png("./test/image_util/raster_page_thumbnail.png").unwrap();

		let expected = std::fs::read("./test/image_util/raster_page_thumbnail.png").unwrap();
		let actual = thumbnail.save_to_png_bytes();
		assert_eq!(
			&*actual, &expected,
			"Raster thumbnail does not match golden master"
		);
	}
}
