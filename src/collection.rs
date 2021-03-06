/*! A collection of song files
 *
 * The actual files are managed in here.
 */
use crate::*;
use anyhow::Context;
use derive_more::*;

use adw::prelude::*;
use gdk::{cairo, gdk_pixbuf};
use gtk::{gdk, gio, glib, glib::clone, prelude::*};
use gtk4 as gtk;
use libadwaita as adw;

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{serde_as, DisplayFromStr};
use std::{
	collections::{BTreeMap, HashMap},
	ops::{Deref, DerefMut, RangeInclusive},
	path::Path,
};
use typed_index_collections::TiVec;
use uuid::Uuid;

pub fn load() -> anyhow::Result<HashMap<Uuid, SongFile>> {
	use itertools::Itertools;

	// TODO don't hardcode here
	let xdg = xdg::BaseDirectories::with_prefix("dinoscore")?;
	xdg.find_data_files("songs")
		.flat_map(|dir| walkdir::WalkDir::new(dir).follow_links(true))
		.filter_ok(|entry| entry.file_type().is_file())
		.map_ok(walkdir::DirEntry::into_path)
		.filter_ok(|path| path.extension() == Some(std::ffi::OsStr::new("zip")))
		.map(|path| {
			let path = path?;
			let song = SongFile::new(&path)
				.context(anyhow::format_err!("Could not load '{}'", path.display()))?;
			Ok((*song.uuid(), song))
		})
		.collect()
}

#[derive(Debug)]
pub struct SongFile {
	file: zip::read::ZipArchive<std::fs::File>,
	pub index: SongMeta,
	thumbnail: Option<gdk_pixbuf::Pixbuf>,
}

impl SongFile {
	pub fn uuid(&self) -> &Uuid {
		&self.index.song_uuid
	}

	pub fn new(path: impl AsRef<Path>) -> anyhow::Result<Self> {
		let path = path.as_ref();
		let mut song = zip::read::ZipArchive::new(std::fs::File::open(path)?)?;

		let (mut index, mut song): (SongMeta, _) = {
			let index: SongMetaVersioned = pipeline::pipe!(
				song.by_name("staves.json")?
				=> std::io::BufReader::new
				=> serde_json::from_reader(_)?
			);
			/* Backwards compatibility handling */
			let index: SongMeta = match index.update() {
				either::Either::Left(index) => index,
				either::Either::Right(index) => {
					let pdf = {
						let mut pages = song.by_name("sheet.pdf")?;
						let mut data: Vec<u8> = vec![];
						std::io::copy(&mut pages, &mut data)?;
						let data = glib::Bytes::from_owned(data);
						std::mem::drop(pages);
						poppler::Document::from_bytes(&data, None)?
					};
					index.update(&pdf)
				},
			};

			(index, song)
		};
		if index.title.is_none() {
			index.title = path
				.file_stem()
				.map(|name| name.to_string_lossy().to_string());
		}

		let thumbnail: Option<gdk_pixbuf::Pixbuf> = song
			.by_name("thumbnail")
			.map(Option::Some)
			.or_else(|e| match e {
				zip::result::ZipError::FileNotFound => Ok(None),
				e => Err(e),
			})
			.transpose() /* Option<Result<_>> */
			.map(|stream| -> anyhow::Result<_> {
				let mut stream = stream?;
				let mut bytes = Vec::new();
				std::io::copy(&mut stream, &mut bytes)?;

				pipeline::pipe! {
					bytes
					=> &glib::Bytes::from_owned
					=> &gio::MemoryInputStream::from_bytes
					=> gdk_pixbuf::Pixbuf::from_stream(_, Option::<&gio::Cancellable>::None)
					=> _.map_err(Into::into)
				}
			})
			.transpose() /* Result<Option<_>> */
			.context("Could not load thumbnail")?;

		Ok(SongFile {
			file: song,
			index,
			thumbnail,
		})
	}

	fn load_sheets_legacy<T>(
		pages: &mut zip::read::ZipFile<'_>,
		mapper: impl Fn(Vec<u8>, poppler::Page) -> T,
	) -> anyhow::Result<TiVec<PageIndex, T>> {
		log::debug!("Loading legacy sheets");
		let mut data: Vec<u8> = vec![];
		std::io::copy(pages, &mut data).context("Failed to load data")?;
		image_util::explode_pdf(&data, mapper).map(Into::into)
	}

	pub fn load_pages<T>(
		&mut self,
		loader: impl Fn(usize, &str, Vec<u8>) -> anyhow::Result<T>,
	) -> anyhow::Result<TiVec<PageIndex, T>> {
		let files_pre_filtered = self
			.file
			.file_names()
			.filter(|name| name.starts_with("page_"))
			.map(str::to_owned)
			.collect::<HashSet<_>>();
		(0..self.index.n_pages)
			.into_iter()
			.map(|index| {
				(|| {
					let index: usize = index;
					let name_prefix = format!("page_{}.", index);
					let matching_files = files_pre_filtered
						.iter()
						.filter(|name| name.starts_with(&name_prefix))
						.collect::<Vec<_>>();
					anyhow::ensure!(!matching_files.is_empty(), "'page_{}.*' not found", index);
					anyhow::ensure!(
						matching_files.len() == 1,
						"Multiple contenders for 'page_{}' found: {:?}",
						index,
						matching_files
					);
					let file = matching_files[0];

					let mut data: Vec<u8> = vec![];
					std::io::copy(&mut self.file.by_name(file)?, &mut data)
						.context("Failed to read data")?;
					loader(index, file, data)
				})()
				.context(anyhow::format_err!("Failed to load page {}", index))
			})
			.collect::<anyhow::Result<_>>()
	}

	pub fn load_sheets(&mut self) -> anyhow::Result<TiVec<PageIndex, PageImageBox>> {
		/* Legacy code path */
		if let Ok(mut pages) = self.file.by_name("sheet.pdf") {
			return Self::load_sheets_legacy(&mut pages, |_, page| Box::new(page) as PageImageBox);
		}

		let pages: TiVec<PageIndex, PageImageBox> = self.load_pages(|index, file, data| {
			let data = glib::Bytes::from_owned(data);

			if file == format!("page_{}.pdf", index) {
				let pdf =
					poppler::Document::from_bytes(&data, None).context("Failed to load PDF")?;
				anyhow::ensure!(
					pdf.n_pages() == 1,
					"Each PDF file must have exactly one page"
				);
				Ok(Box::new(pdf.page(0).unwrap()) as PageImageBox)
			} else {
				Ok(Box::new(
					gdk_pixbuf::Pixbuf::from_stream(
						&gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(data)),
						Option::<&gio::Cancellable>::None,
					)
					.context("Failed to load image")?,
				) as PageImageBox)
			}
		})?;

		anyhow::ensure!(!pages.is_empty(), "No pages found");
		Ok(pages)
	}

	pub fn load_sheets_raw(&mut self) -> anyhow::Result<TiVec<PageIndex, RawPageImage>> {
		/* Legacy code path */
		if let Ok(mut pages) = self.file.by_name("sheet.pdf") {
			return Self::load_sheets_legacy(&mut pages, |raw, page| RawPageImage::Vector {
				raw,
				page,
			});
		}

		let pages: TiVec<PageIndex, RawPageImage> = self.load_pages(|index, file, raw| {
			let data = glib::Bytes::from_owned(raw.clone());

			if file == format!("page_{}.pdf", index) {
				let pdf =
					poppler::Document::from_bytes(&data, None).context("Failed to load PDF")?;
				anyhow::ensure!(
					pdf.n_pages() == 1,
					"Each PDF file must have exactly one page"
				);
				Ok(RawPageImage::Vector {
					page: pdf.page(0).unwrap(),
					raw,
				})
			} else {
				let extension = file
					.split('.')
					.last()
					.ok_or_else(|| anyhow::format_err!("File name for needs to have an extension"))?
					.to_owned();
				Ok(RawPageImage::Raster {
					image: pipeline::pipe! {
						data
						=> &glib::Bytes::from_owned
						=> &gio::MemoryInputStream::from_bytes
						=> gdk_pixbuf::Pixbuf::from_stream(_, Option::<&gio::Cancellable>::None)
					}
					.context("Failed to load image")?,
					raw,
					extension,
				})
			}
		})?;

		anyhow::ensure!(!pages.is_empty(), "No pages found");
		Ok(pages)
	}

	pub fn title(&self) -> Option<&str> {
		self.index.title.as_deref()
	}

	pub fn thumbnail(&self) -> Option<&gdk_pixbuf::Pixbuf> {
		self.thumbnail.as_ref() //.map(fragile::Fragile::get)
	}

	pub fn save<'a, P: AsRef<std::path::Path>>(
		path: P,
		metadata: SongMeta,
		pages: impl IntoIterator<Item = &'a RawPageImage>,
		thumbnail: Option<gdk_pixbuf::Pixbuf>,
		overwrite: bool,
	) -> anyhow::Result<()> {
		let pages = pages.into_iter();

		let file = std::fs::OpenOptions::new()
			.write(true)
			.create_new(!overwrite)
			.create(overwrite)
			.truncate(overwrite)
			.open(&path)
			.context(format!("Could not open file {:?}", path.as_ref().display()))?;
		let mut writer = zip::ZipWriter::new(file);

		writer.start_file("staves.json", zip::write::FileOptions::default())?;
		serde_json::to_writer_pretty(&mut writer, &SongMetaVersioned::from(metadata))?;

		log::info!("Saving sheets");
		for (index, page) in pages.enumerate() {
			writer.start_file(
				format!("page_{}.{}", index, page.extension()),
				zip::write::FileOptions::default(),
			)?;
			use std::io::Write;
			writer.write_all(page.raw())?;
		}

		if let Some(thumbnail) = thumbnail {
			log::info!("Saving thumbnail");
			writer.start_file("thumbnail", zip::write::FileOptions::default())?;

			let buffer = thumbnail.save_to_bufferv("png", &[])?;
			use std::io::Write;
			writer.write_all(&buffer)?;
		}

		writer.finish()?;
		Ok(())
	}

	pub fn generate_thumbnail<'a>(
		song: &SongMeta,
		pages: impl IntoIterator<Item = &'a (impl PageImage + 'a)>,
	) -> cairo::Result<Option<gdk_pixbuf::Pixbuf>> {
		let mut pages = pages.into_iter();
		let staff = if let Some(staff) = song.staves.first() {
			staff
		} else {
			return Ok(None);
		};
		let page: &dyn PageImage = if let Some(page) = pages.nth(*staff.page) {
			page
		} else {
			return Ok(None);
		};

		let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, 400, 100)?;
		let context = cairo::Context::new(&surface)?;

		let scale = surface.width() as f64 / staff.width();
		context.scale(scale, scale);

		context.translate(-staff.left(), -staff.top());
		context.set_source_rgb(1.0, 1.0, 1.0);
		context.paint()?;
		page.render(&context)?;

		surface.flush();

		Ok(Some(
			gdk::pixbuf_get_from_surface(&surface, 0, 0, surface.width(), surface.height())
				.unwrap(),
		))
	}
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SectionMeta {
	pub is_repetition: bool,
	pub section_end: bool,
}

#[derive(
	Debug,
	Display,
	Serialize,
	Deserialize,
	Clone,
	Copy,
	From,
	FromStr,
	Into,
	AsRef,
	AsMut,
	Deref,
	Add,
	Sub,
	PartialEq,
	Eq,
	PartialOrd,
	Ord,
)]
pub struct StaffIndex(pub usize);

#[derive(
	Debug,
	Display,
	Serialize,
	Deserialize,
	Clone,
	Copy,
	From,
	FromStr,
	Into,
	AsRef,
	AsMut,
	Deref,
	Add,
	AddAssign,
	Sub,
	SubAssign,
	PartialEq,
	Eq,
	PartialOrd,
	Ord,
	Hash,
)]
pub struct PageIndex(pub usize);

pub type SongMeta = SongMetaV3;

impl SongMeta {
	pub fn sections(&self) -> Vec<(RangeInclusive<StaffIndex>, bool)> {
		let mut sections = Vec::new();
		let mut iter = self.section_starts.iter().peekable();
		while let Some((key, value)) = iter.next() {
			let start = *key;
			let end = iter
				.peek()
				.map(|(key, value)| {
					if value.section_end {
						**key
					} else {
						**key - 1.into()
					}
				})
				.unwrap_or_else(|| StaffIndex(self.staves.len() - 1));
			sections.push((start..=end, value.is_repetition));
		}
		sections
	}

	/// Convert absolute staff numbers into a (page, staff) pair.
	/// Page indices are relative to the current piece's start. Indices start at 0.
	pub fn page_of_piece(&self, index: StaffIndex) -> (PageIndex, StaffIndex) {
		let piece_start = *self.piece_starts.range(..=&index).next_back().unwrap().0;
		let page = self.staves[index].page - self.staves[piece_start].page;
		let page_staff = self.staves[piece_start..=index]
			.iter()
			.rev()
			.take_while(|staff| staff.page == self.staves[index].page)
			.count();

		(page, page_staff.into())
	}
}

/* Check invariants after deserialization */
impl<'de> Deserialize<'de> for SongMeta {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let unchecked = SongMeta::deserialize(deserializer)?;
		if unchecked.staves.is_empty() {
			return Err(de::Error::custom(
				"Invalid data: Song must have at least one staff",
			));
		}
		for staff in &unchecked.staves {
			if staff.page > PageIndex(unchecked.n_pages) {
				return Err(de::Error::custom(format!(
					"Invalid data: Page index out of bounds: {}, len {}",
					staff.page, unchecked.n_pages
				)));
			}
		}
		if !unchecked.piece_starts.contains_key(&0.into()) {
			return Err(de::Error::custom(
				"Invalid data: Song must start with a piece",
			));
		}
		if !unchecked.section_starts.contains_key(&0.into()) {
			return Err(de::Error::custom(
				"Invalid data: Song must start with a section",
			));
		}
		Ok(unchecked)
	}
}

impl Serialize for SongMeta {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		SongMeta::serialize(self, serializer)
	}
}

// Remove once https://github.com/serde-rs/serde/issues/1183 is closed
#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(remote = "Self")] /* Call custom ser/de for invariants checking */
pub struct SongMetaV3 {
	pub n_pages: usize,
	pub staves: TiVec<StaffIndex, Staff>,
	#[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
	pub piece_starts: BTreeMap<StaffIndex, String>,
	/// The bool tells if it is a repetition or not
	#[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
	pub section_starts: BTreeMap<StaffIndex, SectionMeta>,
	/// A unique identifier for this song that is stable across file modifications
	pub song_uuid: Uuid,
	/// Effectively a random string generated on each save. Useful for caching
	pub version_uuid: Uuid,
	pub title: Option<String>,
	pub composer: Option<String>,
}

// Remove once https://github.com/serde-rs/serde/issues/1183 is closed
#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SongMetaV2 {
	pub staves: Vec<Staff>,
	#[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
	pub piece_starts: BTreeMap<StaffIndex, Option<String>>,
	/// The bool tells if it is a repetition or not
	#[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
	pub section_starts: BTreeMap<StaffIndex, SectionMeta>,
	/// A unique identifier for this song that is stable across file modifications
	pub song_uuid: Uuid,
	/// Effectively a random string generated on each save. Useful for caching
	pub version_uuid: Uuid,
	pub title: Option<String>,
	pub composer: Option<String>,
}

// Remove once https://github.com/serde-rs/serde/issues/1183 is closed
#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
struct SongMetaV1 {
	pub staves: Vec<Line>,
	#[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
	pub piece_starts: BTreeMap<StaffIndex, Option<String>>,
	#[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
	pub section_starts: BTreeMap<StaffIndex, SectionMeta>,
}

// Remove once https://github.com/serde-rs/serde/issues/1183 is closed
#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
struct SongMetaV0 {
	pub staves: Vec<Line>,
	#[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
	pub piece_starts: BTreeMap<StaffIndex, Option<String>>,
	/// The bool tells if it is a repetition or not
	#[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
	pub section_starts: BTreeMap<StaffIndex, bool>,
}

trait UpdateSongMeta {
	fn update(self, pdf: &poppler::Document) -> SongMeta;
}

impl UpdateSongMeta for SongMetaV2 {
	fn update(self, pdf: &poppler::Document) -> SongMeta {
		SongMetaV3 {
			n_pages: pdf.n_pages() as usize,
			staves: self.staves.into(),
			piece_starts: self
				.piece_starts
				.into_iter()
				.map(|(k, v)| (k, v.unwrap_or_default()))
				.collect(),
			section_starts: self.section_starts,
			song_uuid: self.song_uuid,
			version_uuid: self.version_uuid,
			composer: self.composer,
			title: self.title,
		}
	}
}

impl UpdateSongMeta for SongMetaV1 {
	fn update(self, pdf: &poppler::Document) -> SongMeta {
		SongMetaV2 {
			staves: self
				.staves
				.iter()
				.map(|staff| {
					let page = pdf.page(staff.page.0 as i32).unwrap();
					// Convert from relative sizes back to pixels
					let scale_x = page.size().0 as f64;
					let scale_y = page.size().1 as f64;

					StaffV2 {
						page: staff.page,
						start: (staff.start.0 * scale_x, staff.start.1 * scale_y),
						end: (staff.end.0 * scale_x, staff.end.1 * scale_y),
					}
				})
				.collect(),
			piece_starts: self.piece_starts,
			section_starts: self.section_starts,
			/* At each conversion, a new UUID will be chosen. Therefore, the result should be saved. */
			song_uuid: Uuid::new_v4(),
			version_uuid: Uuid::new_v4(),
			composer: None,
			title: None,
		}
		.update(pdf)
	}
}

impl UpdateSongMeta for SongMetaV0 {
	fn update(self, pdf: &poppler::Document) -> SongMeta {
		SongMetaV1 {
			staves: self.staves,
			piece_starts: self.piece_starts,
			section_starts: self
				.section_starts
				.into_iter()
				.map(|(key, is_repetition)| {
					(
						key,
						SectionMeta {
							is_repetition,
							section_end: false,
						},
					)
				})
				.collect(),
		}
		.update(pdf)
	}
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "version")]
enum SongMetaVersioned {
	// The newest variant is always called "V" to reduce renamings
	#[serde(rename = "3")]
	V(SongMetaV3),
	#[serde(rename = "2")]
	V2(SongMetaV2),
	#[serde(rename = "1")]
	V1(SongMetaV1),
	#[serde(rename = "0")]
	V0(SongMetaV0),
}

impl SongMetaVersioned {
	/// Happy case for when no update is needed
	fn update(self) -> either::Either<SongMeta, impl UpdateSongMeta> {
		match self {
			SongMetaVersioned::V(meta) => either::Either::Left(meta),
			_ => either::Either::Right(self),
		}
	}
}

impl UpdateSongMeta for SongMetaVersioned {
	fn update(self, pdf: &poppler::Document) -> SongMeta {
		match self {
			SongMetaVersioned::V(meta) => meta,
			SongMetaVersioned::V2(meta) => meta.update(pdf),
			SongMetaVersioned::V1(meta) => meta.update(pdf),
			SongMetaVersioned::V0(meta) => meta.update(pdf),
		}
	}
}

impl From<SongMeta> for SongMetaVersioned {
	fn from(meta: SongMeta) -> Self {
		SongMetaVersioned::V(meta)
	}
}

pub type Staff = StaffV2;

// Absolute coordinates, on the PDF, in the unit of the PDF
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StaffV2 {
	pub page: PageIndex,
	pub start: (f64, f64),
	pub end: (f64, f64),
}

impl Staff {
	pub fn width(&self) -> f64 {
		self.end.0 - self.start.0
	}
	pub fn height(&self) -> f64 {
		self.end.1 - self.start.1
	}
	pub fn aspect_ratio(&self) -> f64 {
		self.height() / self.width()
	}
	pub fn left(&self) -> f64 {
		self.start.0
	}
	pub fn right(&self) -> f64 {
		self.end.0
	}
	pub fn top(&self) -> f64 {
		self.start.1
	}
	pub fn bottom(&self) -> f64 {
		self.end.1
	}
}

pub type Line = StaffV1;

// Relative coordinates
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StaffV1 {
	pub page: PageIndex,
	pub start: (f64, f64),
	pub end: (f64, f64),
}

impl StaffV1 {
	pub fn get_width(&self) -> f64 {
		self.end.0 - self.start.0
	}

	pub fn get_height(&self) -> f64 {
		self.end.1 - self.start.1
	}
}
