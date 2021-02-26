/*! A collection of song files
 * 
 * The actual files are managed in here.
 */
use std::path::Path;
use uuid::Uuid;
use derive_more::*;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{serde_as, DisplayFromStr};
use std::{
	collections::{BTreeMap, HashMap},
	ops::{Deref, DerefMut, RangeInclusive},
};
use crate::owned;

pub async fn load() -> HashMap<Uuid, SongFile> {
	use futures::StreamExt;

	let xdg = xdg::BaseDirectories::with_prefix("dinoscore").unwrap();
	futures::stream::iter(xdg.list_data_files("songs"))
		.filter(|path| futures::future::ready(path.is_file()))
		.then(|path| async move {
			let song = SongFile::new(&path).await;
			(*song.uuid(), song)
		})
		.collect()
		.await
}

#[derive(Debug)]
pub struct SongFile {
	file: zip::read::ZipArchive<std::fs::File>,
	pub index: SongMeta,
	thumbnail: Option<fragile::Fragile<gdk_pixbuf::Pixbuf>>,
}

impl SongFile {
	pub fn uuid(&self) -> &Uuid {
		&self.index.song_uuid
	}

	pub async fn new(
		path: impl AsRef<Path>,
	) -> Self {
		let path = path.as_ref();
		let mut song = zip::read::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();

		let (mut index, mut song): (SongMeta, _) = async_std::task::spawn_blocking(move || {
			let index: SongMetaVersioned = serde_json::from_reader(song.by_name("staves.json").unwrap()).unwrap();
			let index: SongMeta = match index.update() {
				either::Either::Left(index) => index,
				either::Either::Right(index) => {
					let pdf = {
						let mut pages = song.by_name("sheet.pdf").unwrap();
						let mut data: Vec<u8> = vec![];
						std::io::copy(&mut pages, &mut data).unwrap();
						let data = glib::Bytes::from_owned(data);
						std::mem::drop(pages);
						poppler::PopplerDocument::new_from_bytes(data, "").unwrap()
					};
					index.update(&pdf)
				},
			};

			(index, song)
		}).await;
		if index.title.is_none() {
			index.title = path.file_stem().map(|name| name.to_string_lossy().to_string());
		}

		let (song, thumbnail) = async_std::task::spawn_blocking(move || {
			let pixbuf: Option<owned::OwnedPixbuf> = song.by_name("thumbnail")
				.map(Option::Some)
				.or_else(|e| match e {
					zip::result::ZipError::FileNotFound => Ok(None),
					e => Err(e)
				})
				.transpose()
				.map(|opt| opt.map(|mut stream| {
					let mut bytes = Vec::new();
					std::io::copy(&mut stream, &mut bytes).unwrap();
					let pixbuf = gdk_pixbuf::Pixbuf::from_stream(
						&gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes)),
						Option::<&gio::Cancellable>::None
					).unwrap();
					unsafe {owned::OwnedPixbuf::from_inner_raw(pixbuf)}
				}))
				.map(Result::unwrap);
			(song, pixbuf)
		}).await;

		SongFile {
			file: song,
			index,
			thumbnail: thumbnail.map(|thumbnail| fragile::Fragile::new(thumbnail.into_inner())),
			// staves: futures::stream::iter(metadata.staves.iter().enumerate())
			// 	.then(|(idx, line)| {
			// 		// TODO song content versioning
			// 		Staff::new_from_pdf(
			// 			pages.get_page(line.page.into()).unwrap(),
			// 			line,
			// 			idx,
			// 			image_cache.clone(),
			// 			path.file_name().unwrap(),
			// 			0,
			// 		)
			// 	})
			// 	.collect()
			// 	.await,
			// sections: metadata.sections(),
			// piece_starts: metadata.0.piece_starts,
		}
	}

	pub async fn load_sheet_async(mut self) -> (Self, owned::OwnedPopplerDocument) {
		async_std::task::spawn_blocking(|| {
			let mut pages = self.file.by_name("sheet.pdf").unwrap();
			let mut data: Vec<u8> = vec![];
			std::io::copy(&mut pages, &mut data).unwrap();
			let data = glib::Bytes::from_owned(data);
			std::mem::drop(pages);
			(self, owned::OwnedPopplerDocument::new_from_bytes(data, "").unwrap())
		}).await
	}

	pub fn load_sheet(&mut self) -> owned::OwnedPopplerDocument {
		let mut pages = self.file.by_name("sheet.pdf").unwrap();
		let mut data: Vec<u8> = vec![];
		std::io::copy(&mut pages, &mut data).unwrap();
		let data = glib::Bytes::from_owned(data);
		std::mem::drop(pages);
		owned::OwnedPopplerDocument::new_from_bytes(data, "").unwrap()
	}

	pub fn title(&self) -> Option<&str> {
		self.index.title.as_deref()
	}

	pub fn thumbnail(&self) -> Option<&gdk_pixbuf::Pixbuf> {
		self.thumbnail.as_ref().map(fragile::Fragile::get)
	}

	pub fn save<'a, P: AsRef<std::path::Path>>(
		path: P,
		metadata: SongMeta,
		pages: impl Iterator<Item = maybe_owned::MaybeOwned<'a, poppler::PopplerPage>>,
		thumbnail: Option<gdk_pixbuf::Pixbuf>,
		overwrite: bool,
	) {
		let file = std::fs::OpenOptions::new()
			.write(true)
			.create_new(!overwrite)
			.create(overwrite)
			.truncate(overwrite)
			.open(path)
			.unwrap();
		let mut writer = zip::ZipWriter::new(file);

		writer
			.start_file("staves.json", zip::write::FileOptions::default())
			.unwrap();
		serde_json::to_writer(&mut writer, &SongMetaVersioned::from(metadata)).unwrap();

		{
			println!("Saving sheets");

			writer
				.start_file("sheet.pdf", zip::write::FileOptions::default())
				.unwrap();
			let surface = cairo::PdfSurface::for_stream(500.0, 500.0, writer).unwrap();
			let context = cairo::Context::new(&surface);
			for page in pages {
				surface
					.set_size(page.get_size().0, page.get_size().1)
					.unwrap();
				page.render(&context);
				context.show_page();
			}
			surface.flush();
			writer = *surface
				.finish_output_stream()
				.unwrap()
				.downcast::<zip::ZipWriter<std::fs::File>>()
				.unwrap();
		}

		if let Some(thumbnail) = thumbnail {
			println!("Saving thumbnail");
			writer
				.start_file("thumbnail", zip::write::FileOptions::default())
				.unwrap();

			let buffer = thumbnail.save_to_bufferv("png", &[]).unwrap();
			use std::io::Write;
			writer.write_all(&buffer).unwrap();
		}
	
		writer.finish().unwrap();
	}

	// TODO make associated method
	pub fn generate_thumbnail<'a>(song: &SongMeta, mut pages: impl Iterator<Item = maybe_owned::MaybeOwned<'a, poppler::PopplerPage>>) -> Option<gdk_pixbuf::Pixbuf> {
		let staff = song.staves.first()?;
		let page = pages.nth(*staff.page)?;

		let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, 400, 100).unwrap();
		let context = cairo::Context::new(&surface);

		let scale = surface.get_width() as f64 / staff.width();
		context.scale(scale, scale);

		context.translate(
			-staff.left(),
			-staff.top(),
		);
		context.set_source_rgb(1.0, 1.0, 1.0);
		context.paint();
		page.render(&context);

		surface.flush();

		Some(gdk::pixbuf_get_from_surface(&surface, 0, 0, surface.get_width(), surface.get_height()).unwrap())
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
	Sub,
	PartialEq,
	Eq,
	PartialOrd,
	Ord,
)]
pub struct PageIndex(pub usize);

pub type SongMeta = SongMetaV2;

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
}

/* Check invariants after deserialization */
impl<'de> Deserialize<'de> for SongMeta {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let unchecked = SongMeta::deserialize(deserializer)?;
		if unchecked.staves.is_empty() {
			return Err(de::Error::custom("song must have at least one staff"));
		}
		if !unchecked.piece_starts.contains_key(&0.into()) {
			return Err(de::Error::custom("song must start with a piece"));
		}
		if !unchecked.section_starts.contains_key(&0.into()) {
			return Err(de::Error::custom("song must start with a section"));
		}
		Ok(unchecked)
	}
}

impl Serialize for SongMeta {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		SongMeta::serialize(&self, serializer)
	}
}

// Remove once https://github.com/serde-rs/serde/issues/1183 is closed
#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(remote = "Self")] /* Call custom ser/de for invariants checking */
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
	fn update(self, pdf: &poppler::PopplerDocument) -> SongMeta;
}

impl UpdateSongMeta for SongMetaV1 {
	fn update(self, pdf: &poppler::PopplerDocument) -> SongMeta {
		SongMetaV2 {
			staves: self.staves.iter()
				.map(|staff| {
					let page = pdf.get_page(staff.page.0).unwrap();
					// Convert from relative sizes back to pixels
					let scale_x = page.get_size().0 as f64;
					let scale_y = page.get_size().1 as f64;

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
	}
}

impl UpdateSongMeta for SongMetaV0 {
	fn update(self, pdf: &poppler::PopplerDocument) -> SongMeta {
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
		}.update(pdf)
	}
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "version")]
enum SongMetaVersioned {
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
			SongMetaVersioned::V2(meta) => either::Either::Left(meta),
			_ => either::Either::Right(self),
		}
	}
}

impl UpdateSongMeta for SongMetaVersioned {
	fn update(self, pdf: &poppler::PopplerDocument) -> SongMeta {
		match self {
			SongMetaVersioned::V2(meta) => meta,
			SongMetaVersioned::V1(meta) => meta.update(&pdf),
			SongMetaVersioned::V0(meta) => meta.update(&pdf),
		}
	}
}

impl From<SongMeta> for SongMetaVersioned {
	fn from(meta: SongMeta) -> Self {
		SongMetaVersioned::V2(meta)
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
