#![allow(unused_variables, clippy::too_many_arguments)]

use gdk_pixbuf::{Colorspace, InterpType, Pixbuf, PixbufRotation};
use poppler::{PopplerDocument, PopplerPage};

#[derive(Debug)]
pub struct OwnedPixbuf(Pixbuf);

/* This would be unsafe if it was public */
impl OwnedPixbuf {
	fn map<T>(&self, function: impl Fn(&Pixbuf) -> T) -> T {
		function(&self.0)
	}

	fn map_mut<T>(&mut self, function: impl Fn(&Pixbuf) -> T) -> T {
		function(&self.0)
	}

	fn map_into<T>(self, function: impl Fn(Pixbuf) -> T) -> T {
		function(self.0)
	}

	pub fn from_inner(pixbuf: Pixbuf) -> OwnedPixbuf {
		todo!("Clone that pixbuf first (deeply)")
	}

	/// Safety guarantee: for this to be safe, you *must* be the unique
	/// owner of the passed PixBuf. You must not have made any clones of it
	/// nor any other references to it.
	pub unsafe fn from_inner_raw(pixbuf: Pixbuf) -> OwnedPixbuf {
		OwnedPixbuf(pixbuf)
	}

	pub fn into_inner(self) -> Pixbuf {
		self.0
	}
}

unsafe impl Send for OwnedPixbuf {}

impl std::fmt::Display for OwnedPixbuf {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		write!(f, "OwnedPixbuf")
	}
}

/* This block merely mirrors the public API */
impl OwnedPixbuf {
	pub fn new(
		colorspace: Colorspace,
		has_alpha: bool,
		bits_per_sample: i32,
		width: i32,
		height: i32,
	) -> Option<OwnedPixbuf> {
		Pixbuf::new(colorspace, has_alpha, bits_per_sample, width, height).map(OwnedPixbuf)
	}

	pub fn from_bytes(
		data: impl AsRef<[u8]> + Send + 'static,
		colorspace: Colorspace,
		has_alpha: bool,
		bits_per_sample: i32,
		width: i32,
		height: i32,
		rowstride: i32,
	) -> OwnedPixbuf {
		OwnedPixbuf(Pixbuf::from_bytes(
			&glib::Bytes::from_owned(data),
			colorspace,
			has_alpha,
			bits_per_sample,
			width,
			height,
			rowstride,
		))
	}

	pub fn from_resource(resource_path: &str) -> Result<OwnedPixbuf, glib::Error> {
		Pixbuf::from_resource(resource_path).map(OwnedPixbuf)
	}

	pub fn from_resource_at_scale(
		resource_path: &str,
		width: i32,
		height: i32,
		preserve_aspect_ratio: bool,
	) -> Result<OwnedPixbuf, glib::Error> {
		Pixbuf::from_resource_at_scale(resource_path, width, height, preserve_aspect_ratio)
			.map(OwnedPixbuf)
	}

	pub fn from_xpm_data(data: &[&str]) -> OwnedPixbuf {
		OwnedPixbuf(Pixbuf::from_xpm_data(data))
	}

	pub fn add_alpha(self, substitute_color: bool, r: u8, g: u8, b: u8) -> Option<OwnedPixbuf> {
		self.map_into(|pixbuf| pixbuf.add_alpha(substitute_color, r, g, b))
			.map(OwnedPixbuf)
	}

	pub fn apply_embedded_orientation(self) -> Option<OwnedPixbuf> {
		self.map_into(|pixbuf| pixbuf.apply_embedded_orientation())
			.map(OwnedPixbuf)
	}

	pub fn composite(
		&mut self,
		dest: &Pixbuf,
		dest_x: i32,
		dest_y: i32,
		dest_width: i32,
		dest_height: i32,
		offset_x: f64,
		offset_y: f64,
		scale_x: f64,
		scale_y: f64,
		interp_type: InterpType,
		overall_alpha: i32,
	) {
		todo!()
	}

	pub fn composite_color_simple(
		&self,
		dest_width: i32,
		dest_height: i32,
		interp_type: InterpType,
		overall_alpha: i32,
		check_size: i32,
		color1: u32,
		color2: u32,
	) -> Option<OwnedPixbuf> {
		self.map(|pixbuf| {
			pixbuf.composite_color_simple(
				dest_width,
				dest_height,
				interp_type,
				overall_alpha,
				check_size,
				color1,
				color2,
			)
		})
		.map(OwnedPixbuf)
	}

	pub fn fill(&mut self, pixel: u32) {
		self.map_mut(|pixbuf| pixbuf.fill(pixel));
	}

	pub fn flip(&self, horizontal: bool) -> Option<OwnedPixbuf> {
		self.map(|pixbuf| pixbuf.flip(horizontal)).map(OwnedPixbuf)
	}

	pub fn get_bits_per_sample(&self) -> i32 {
		self.map(|pixbuf| pixbuf.get_bits_per_sample())
	}

	pub fn get_byte_length(&self) -> usize {
		self.map(|pixbuf| pixbuf.get_byte_length())
	}

	pub fn get_colorspace(&self) -> Colorspace {
		self.map(|pixbuf| pixbuf.get_colorspace())
	}

	pub fn get_has_alpha(&self) -> bool {
		self.map(|pixbuf| pixbuf.get_has_alpha())
	}

	pub fn get_height(&self) -> i32 {
		self.map(|pixbuf| pixbuf.get_height())
	}

	pub fn get_n_channels(&self) -> i32 {
		self.map(|pixbuf| pixbuf.get_n_channels())
	}

	pub fn get_option(&self, key: &str) -> Option<&str> {
		todo!()
	}

	pub fn get_rowstride(&self) -> i32 {
		self.map(|pixbuf| pixbuf.get_rowstride())
	}

	pub fn get_width(&self) -> i32 {
		self.map(|pixbuf| pixbuf.get_width())
	}

	pub fn read_pixel_bytes(&self) -> Option<glib::Bytes> {
		self.map(|pixbuf| pixbuf.read_pixel_bytes())
	}

	pub fn remove_option(&mut self, key: &str) -> bool {
		self.map_mut(|pixbuf| pixbuf.remove_option(key))
	}

	pub fn rotate_simple(&self, angle: PixbufRotation) -> Option<OwnedPixbuf> {
		self.map(|pixbuf| pixbuf.rotate_simple(angle))
			.map(OwnedPixbuf)
	}

	pub fn set_option(&mut self, key: &str, value: &str) -> bool {
		self.map_mut(|pixbuf| pixbuf.set_option(key, value))
	}

	pub fn get_property_pixel_bytes(&self) -> Option<glib::Bytes> {
		self.read_pixel_bytes()
	}
}

#[derive(Debug)]
pub struct OwnedPopplerDocument(PopplerDocument);

unsafe impl Send for OwnedPopplerDocument {}

impl OwnedPopplerDocument {
	pub fn into_inner(self) -> PopplerDocument {
		self.0
	}
}

impl OwnedPopplerDocument {
	pub fn new_from_file<P: AsRef<std::path::Path>>(
		p: P,
		password: &str,
	) -> Result<OwnedPopplerDocument, glib::error::Error> {
		PopplerDocument::new_from_file(p, password).map(OwnedPopplerDocument)
	}

	pub fn new_from_bytes(
		bytes: impl AsRef<[u8]> + Send + 'static,
		password: &str,
	) -> Result<OwnedPopplerDocument, glib::error::Error> {
		PopplerDocument::new_from_bytes(glib::Bytes::from_owned(bytes), password)
			.map(OwnedPopplerDocument)
	}

	pub fn get_title(&self) -> Option<String> {
		self.0.get_title()
	}

	pub fn get_metadata(&self) -> Option<String> {
		self.0.get_metadata()
	}

	pub fn get_pdf_version_string(&self) -> Option<String> {
		self.0.get_pdf_version_string()
	}

	pub fn get_permissions(&self) -> u8 {
		self.0.get_permissions()
	}

	pub fn get_n_pages(&self) -> usize {
		self.0.get_n_pages()
	}

	pub fn get_page(&self, index: usize) -> Option<PopplerPage> {
		self.0.get_page(index)
	}
}
