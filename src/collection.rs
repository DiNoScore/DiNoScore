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
	sync::{Arc, Mutex, MutexGuard},
};
use typed_index_collections::TiVec;
use uuid::Uuid;

/* The HashSet contains the names of all song files with out of date format */
pub fn load() -> anyhow::Result<(HashMap<Uuid, SongFile>, HashSet<String>)> {
	use itertools::Itertools;

	let mut outdated_format = HashSet::new();

	// TODO don't hardcode here
	let xdg = xdg::BaseDirectories::with_prefix("dinoscore")?;
	xdg.find_data_files("songs")
		.flat_map(|dir| walkdir::WalkDir::new(dir).follow_links(true))
		.filter_ok(|entry| entry.file_type().is_file())
		.map_ok(walkdir::DirEntry::into_path)
		.filter_ok(|path| path.extension() == Some(std::ffi::OsStr::new("zip")))
		.map(|path| {
			let path = path?;
			let song = SongFile::new(&path, &mut outdated_format)
				.context(anyhow::format_err!("Could not load '{}'", path.display()))?;
			Ok((*song.uuid(), song))
		})
		.collect::<anyhow::Result<_>>()
		.map(|songs| (songs, outdated_format))
}

#[derive(Debug)]
pub struct SongFile {
	file: Arc<Mutex<zip::read::ZipArchive<std::fs::File>>>,
	pub index: SongMeta,
	thumbnail: Option<gdk_pixbuf::Pixbuf>,
}

impl SongFile {
	pub fn uuid(&self) -> &Uuid {
		&self.index.song_uuid
	}

	pub fn new(
		path: impl AsRef<Path>,
		outdated_format: &mut HashSet<String>,
	) -> anyhow::Result<Self> {
		let path = path.as_ref();
		log::debug!("Loading: {}", path.display());
		let mut song = zip::read::ZipArchive::new(std::fs::File::open(path)?)?;

		let (mut index, mut song): (SongMeta, _) = {
			let index: SongMetaVersioned = pipeline::pipe!(
				song.by_name("staves.json")?
				=> std::io::BufReader::new
				=> serde_json::from_reader(_)?
			);
			/* Backwards compatibility handling */
			use std::cell::RefCell;
			let song = RefCell::new(song);
			let index: SongMeta = index.update(|n_pages| {
				/* Warning: n_pages might be zero in the case of guaranteed legacy path! */

				outdated_format.insert(path.file_name().unwrap().to_string_lossy().to_string());

				let mut song = song.borrow_mut();
				Ok(
					Self::load_pages_inner(&mut song, n_pages, |index, file, data| {
						let extension = file
							.split('.')
							.last()
							.ok_or_else(|| {
								anyhow::format_err!("File name needs to have an extension")
							})?
							.to_owned();
						let image = if extension == "pdf" {
							PageImage::from_pdf(data)?
						} else {
							PageImage::from_image(data, extension)?
						};
						anyhow::Ok((image.reference_width(), image.reference_height()) as (f64, f64))
					})?
					.raw
					.into_boxed_slice(),
				)
			})?;

			(index, song.into_inner())
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
			file: Arc::new(Mutex::new(song)),
			index,
			thumbnail,
		})
	}

	fn load_pages_inner<T>(
		file: &mut zip::ZipArchive<std::fs::File>,
		n_pages: usize,
		loader: impl Fn(usize, &str, Vec<u8>) -> anyhow::Result<T>,
	) -> anyhow::Result<TiVec<PageIndex, T>> {
		/* Warning: n_pages might be zero in the case of guaranteed legacy path! */

		/* Legacy code path */
		if let Ok(mut pages) = file.by_name("sheet.pdf") {
			log::debug!("Loading legacy sheets");
			let mut data: Vec<u8> = vec![];
			std::io::copy(&mut pages, &mut data).context("Failed to load data")?;
			return Ok(image_util::explode_pdf(&data)?
				.enumerate()
				.map(|(index, result)| {
					let (bytes, page) = result?;
					loader(index, "sheet.pdf", bytes)
				})
				.collect::<anyhow::Result<Vec<_>>>()?
				.into());
		}

		let files_pre_filtered = file
			.file_names()
			.filter(|name| name.starts_with("page_"))
			.map(str::to_owned)
			.collect::<HashSet<_>>();
		(0..n_pages)
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
					let file_name = matching_files[0];

					let mut data: Vec<u8> = vec![];
					std::io::copy(&mut file.by_name(file_name)?, &mut data)
						.context("Failed to read data")?;
					loader(index, file_name, data)
				})()
				.context(anyhow::format_err!("Failed to load page {}", index))
			})
			.collect::<anyhow::Result<_>>()
	}

	/* Returns a deferred that should be spawned on a background thread */
	pub fn load_pages<T>(
		&self,
		loader: impl Fn(usize, &str, Vec<u8>) -> anyhow::Result<T>,
	) -> impl (FnOnce() -> anyhow::Result<TiVec<PageIndex, T>>) {
		let file = self.file.clone();
		let n_pages = self.index.n_pages;
		move || Self::load_pages_inner(&mut *file.lock().unwrap(), n_pages, loader)
	}

	/* Returns a deferred that should be spawned on a background thread */
	pub fn load_sheets(&self) -> impl (FnOnce() -> anyhow::Result<TiVec<PageIndex, PageImage>>) {
		let load_pages = self.load_pages(|index, file, data| {
			let extension = file
				.split('.')
				.last()
				.ok_or_else(|| anyhow::format_err!("File name for needs to have an extension"))?
				.to_owned();
			if extension == "pdf" {
				PageImage::from_pdf(data)
			} else {
				PageImage::from_image(data, extension)
			}
		});
		|| {
			let start = std::time::Instant::now();
			let pages: TiVec<PageIndex, PageImage> = load_pages()?;

			anyhow::ensure!(!pages.is_empty(), "No pages found");
			log::debug!("Loading sheets took: {:?}", start.elapsed());
			Ok(pages)
		}
	}

	pub fn title(&self) -> Option<&str> {
		self.index.title.as_deref()
	}

	pub fn thumbnail(&self) -> Option<&gdk_pixbuf::Pixbuf> {
		self.thumbnail.as_ref()
	}

	pub fn save<'a, P: AsRef<std::path::Path>>(
		path: P,
		metadata: SongMeta,
		pages: impl IntoIterator<Item = &'a PageImage>,
		thumbnail: Option<gdk_pixbuf::Pixbuf>,
		overwrite: bool,
	) -> anyhow::Result<()> {
		let pages = pages.into_iter();

		let file = atomicwrites::AtomicFile::new(
			&path,
			if overwrite {
				atomicwrites::AllowOverwrite
			} else {
				atomicwrites::DisallowOverwrite
			},
		);

		file.write(|file| {
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

			anyhow::Ok(())
		})
		.map_err(|err| match err {
			atomicwrites::Error::Internal(err) => anyhow::Error::new(err),
			atomicwrites::Error::User(err) => err,
		})
		.context(format!(
			"Failed to save file at {:?}",
			path.as_ref().display()
		))?;

		Ok(())
	}

	pub fn generate_thumbnail<'a>(
		song: &SongMeta,
		pages: impl IntoIterator<Item = &'a PageImage>,
	) -> cairo::Result<Option<gdk_pixbuf::Pixbuf>> {
		let mut pages = pages.into_iter();
		let staff = if let Some(staff) = song.staves.first() {
			staff
		} else {
			return Ok(None);
		};
		let page: &PageImage = if let Some(page) = pages.nth(*staff.page) {
			page
		} else {
			return Ok(None);
		};

		let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, 400, 100)?;
		let context = cairo::Context::new(&surface)?;

		let scale = surface.width() as f64 / page.reference_width() / staff.width();
		context.scale(scale, scale);

		context.translate(
			-staff.left() * page.reference_width(),
			-staff.top() * page.reference_width(),
		);
		context.set_source_rgb(1.0, 1.0, 1.0);
		context.paint()?;
		page.render_cairo(&context)?;

		surface.flush();

		Ok(Some(
			gdk::pixbuf_get_from_surface(&surface, 0, 0, surface.width(), surface.height())
				.unwrap(),
		))
	}
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default)]
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

pub type SongMeta = SongMetaV4;

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
		assert!(self.staves[index].page >= self.staves[piece_start].page);
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
		unchecked.staves.windows(2)
			.map(|staves| (staves.raw[0].page, staves.raw[1].page))
			.map(|(a, b)| if a > b {
				Err(de::Error::custom(format!(
					"Invalid data: Pages must be monotonically increasing, but a staff with page {b} came after one with page {a}"
				)))
			} else {
				Ok(())
			})
			.collect::<Result<(), _>>()?;

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
		if **unchecked.piece_starts.keys().next_back().unwrap() >= unchecked.staves.len() {
			return Err(de::Error::custom("Invalid data: Piece start out of bounds"));
		}
		if **unchecked.section_starts.keys().next_back().unwrap() >= unchecked.staves.len() {
			return Err(de::Error::custom(
				"Invalid data: Section start out of bounds",
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
#[serde(remote = "Self")] /* Call custom ser/de for invariants checking. ONLY FOR LATEST VERSION! */
pub struct SongMetaV4 {
	pub n_pages: usize,
	pub staves: TiVec<StaffIndex, StaffV3>,
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
pub struct SongMetaV3 {
	pub n_pages: usize,
	pub staves: TiVec<StaffIndex, StaffV2>,
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
	pub staves: Vec<StaffV2>,
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

impl SongMetaV3 {
	fn update(self, page_sizes: &[(f64, f64)]) -> SongMeta {
		log::debug!("Updating file: v3 -> v4");
		/* What changed: staves are in relative coordinates again (but differently, see comment below)
		 * Pages may now explicitly be non-PDF and of varying size
		 */
		SongMetaV4 {
			n_pages: self.n_pages,
			staves: self
				.staves
				.into_iter()
				.map(|staff| {
					// Convert from pixels to normalized coordinates by dividing by the page width
					let scale = 1.0 / page_sizes[staff.page.0].0 as f64;

					StaffV3 {
						page: staff.page,
						start: (staff.start.0 * scale, staff.start.1 * scale),
						end: (staff.end.0 * scale, staff.end.1 * scale),
					}
				})
				.collect(),
			piece_starts: self.piece_starts,
			section_starts: self.section_starts,
			song_uuid: self.song_uuid,
			version_uuid: self.version_uuid,
			composer: self.composer,
			title: self.title,
		}
	}
}

impl SongMetaV2 {
	fn update(self, page_sizes: &[(f64, f64)]) -> SongMeta {
		log::debug!("Updating file: v2 -> v3");
		/* What changed: piece_starts now uses TiVec instead of BTreeMap */
		SongMetaV3 {
			n_pages: page_sizes.len(),
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
		.update(page_sizes)
	}
}

impl SongMetaV1 {
	fn update(self, page_sizes: &[(f64, f64)]) -> SongMeta {
		log::debug!("Updating file: v1 -> v2");
		/* What changed: staves now use absolute instead of relative coordinates.
		 * Added UUID fields. Pages are now page_XX.pdf instead of staves.pdf
		 */
		SongMetaV2 {
			staves: self
				.staves
				.iter()
				.map(|staff| {
					// Convert from relative sizes back to pixels
					let scale_x = page_sizes[staff.page.0].0 as f64;
					let scale_y = page_sizes[staff.page.0].1 as f64;

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
		.update(page_sizes)
	}
}

impl SongMetaV0 {
	fn update(self, page_sizes: &[(f64, f64)]) -> SongMeta {
		log::debug!("Updating file: v0 -> v1");
		/* What changed: section_start representation */
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
		.update(page_sizes)
	}
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "version")]
enum SongMetaVersioned {
	// The newest variant is always called "V" to reduce renamings
	#[serde(rename = "4")]
	V(SongMetaV4),
	#[serde(rename = "3")]
	V3(SongMetaV3),
	#[serde(rename = "2")]
	V2(SongMetaV2),
	#[serde(rename = "1")]
	V1(SongMetaV1),
	#[serde(rename = "0")]
	V0(SongMetaV0),
}

impl SongMetaVersioned {
	fn update(
		self,
		load_page_sizes: impl FnOnce(usize) -> anyhow::Result<Box<[(f64, f64)]>>,
	) -> anyhow::Result<SongMeta> {
		match self {
			SongMetaVersioned::V(meta) => Ok(meta),
			SongMetaVersioned::V3(meta @ SongMetaV3 { n_pages, .. }) => {
				Ok(meta.update(&load_page_sizes(n_pages)?))
			},
			SongMetaVersioned::V2(meta) => Ok(meta.update(&load_page_sizes(0)?)),
			SongMetaVersioned::V1(meta) => Ok(meta.update(&load_page_sizes(0)?)),
			SongMetaVersioned::V0(meta) => Ok(meta.update(&load_page_sizes(0)?)),
		}
	}
}

impl From<SongMeta> for SongMetaVersioned {
	fn from(meta: SongMeta) -> Self {
		SongMetaVersioned::V(meta)
	}
}

/* As you can see, we've gone through quite a few iterations on which coordinate
 * space to use for describing staff coordinates. Here's what didn't work out in the
 * past any why:
 *
 * V1: Relative coordinates. This makes it impossible to calculate the aspect
 * ratio without knowing the actual paper size, and all scaling operations need
 * to be corrected for that aspect ratio mismatch.
 *
 * V2: Absolute coordinates. This mostly works, but it requires tracking the
 * original paper size along everywhere. This makes it annoying to work with
 * when the base images get scaled. Many code assumes that all pages have the
 * same size in their way how the coordinates are calculated with. This breaks
 * in some really subtle ways once that assumption gets violated.
 *
 * V3: Coordinates scaled to a reference paper width of 1. This preserves aspect
 * ratio while remaining independent of the original image size.
 */
pub type Staff = StaffV3;

// Coordinates are scaled to a reference paper width of 1.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StaffV3 {
	pub page: PageIndex,
	pub start: (f64, f64),
	pub end: (f64, f64),
}

impl Staff {
	pub fn page(&self) -> PageIndex {
		self.page
	}
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
	/** "Merge" two staves by calculating their common bounding box.
	 * Only valid for staves of the same page of the same song
	 */
	pub fn merge(&self, other: &Self) -> Self {
		assert_eq!(self.page, other.page);
		Self {
			page: self.page,
			start: (
				self.start.0.min(other.start.0),
				self.start.1.min(other.start.1),
			),
			end: (self.end.0.max(other.end.0), self.end.1.max(other.end.1)),
		}
	}
}

// Absolute coordinates, on the page image, in the unit of the current page
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StaffV2 {
	page: PageIndex,
	start: (f64, f64),
	end: (f64, f64),
}

pub type Line = StaffV1;

// Relative coordinates
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StaffV1 {
	page: PageIndex,
	start: (f64, f64),
	end: (f64, f64),
}

impl StaffV1 {
	pub fn get_width(&self) -> f64 {
		self.end.0 - self.start.0
	}

	pub fn get_height(&self) -> f64 {
		self.end.1 - self.start.1
	}
}

#[cfg(test)]
mod test {
	use super::*;

	#[test]
	fn test_format_v2() {
		let song = SongFile::new(&"./test/format_v2.zip", &mut Default::default()).unwrap();
		song.load_sheets()().unwrap();
	}

	#[test]
	fn test_format_v3() {
		let song = SongFile::new(&"./test/format_v3.zip", &mut Default::default()).unwrap();
		song.load_sheets()().unwrap();
	}

	#[test]
	fn test_format_v4() {
		let song = SongFile::new(&"./test/format_v4.zip", &mut Default::default()).unwrap();
		song.load_sheets()().unwrap();
	}
}
