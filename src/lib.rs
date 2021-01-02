use std::cell::RefCell;
use std::rc::Rc;
use std::path::Path;
use std::{
	collections::{BTreeMap, HashMap, HashSet},
	ops::RangeInclusive,
	path::PathBuf,
};

use futures::prelude::*;
use gdk::prelude::*;
use gio::prelude::*;
use gtk::prelude::*;

use blocking::unblock;

pub mod song;
use song::*;

#[derive(Clone, Debug)]
pub struct StaffLayout {
	pub index: StaffIndex,
	pub x: f64,
	pub y: f64,
	pub width: f64,
}

#[derive(Debug)]
pub struct Song {
	pub staves: Vec<Staff>,
	pub piece_starts: BTreeMap<StaffIndex, Option<String>>,
	pub sections: Vec<(RangeInclusive<StaffIndex>, bool)>,
}

impl Song {
	pub async fn new(
		path: impl AsRef<Path>,
		image_cache: Rc<RefCell<lru_disk_cache::LruDiskCache>>,
	) -> Self {
		let path = path.as_ref();
		let mut song =
			zip::read::ZipArchive::new(std::fs::File::open(path).unwrap())
				.unwrap();
		// I'm tired, okay?
		// TODO wtf
		let (pages, mut song) = {
			let (data, song) = unblock! {
				let data = {
					let mut pages = song.by_name("sheet.pdf").unwrap();
					let mut data: Vec<u8> = vec![];
					std::io::copy(&mut pages, &mut data).unwrap();
					let data: &mut [u8] = &mut *Box::leak(data.into_boxed_slice()); // TODO: absolutely remove this
					data
				};
				(data, song)
			};
			(
				poppler::PopplerDocument::new_from_data(data, "").unwrap(),
				song,
			)
		};
		let metadata: SongMeta =
			unblock! { serde_json::from_reader(song.by_name("staves.json").unwrap()).unwrap() };
		Song {
			staves: futures::stream::iter(metadata.staves.iter().enumerate())
				.then(|(idx, line)| {
					// TODO song content versioning
					Staff::new_from_pdf(pages.get_page(line.page.into()).unwrap(), line, idx, image_cache.clone(), path.file_name().unwrap(), 0)
				})
				.collect()
				.await,
			sections: metadata.sections(),
			piece_starts: metadata.0.piece_starts,
		}
	}

	pub async fn load_first_staff(path: impl AsRef<Path>) -> Option<gdk_pixbuf::Pixbuf> {
		// TODO put a proper thumbnaild picture into the zip file
		// So that we don't need to parse/load that whole thing twice
		// (But it will do for now)

		let mut song =
			zip::read::ZipArchive::new(std::fs::File::open(path).unwrap())
				.unwrap();
		// I'm tired, okay?
		// TODO wtf
		let (pages, mut song) = {
			let (data, song) = unblock! {
				let data = {
					let mut pages = song.by_name("sheet.pdf").unwrap();
					let mut data: Vec<u8> = vec![];
					std::io::copy(&mut pages, &mut data).unwrap();
					let data: &mut [u8] = &mut *Box::leak(data.into_boxed_slice()); // TODO: absolutely remove this
					data
				};
				(data, song)
			};
			(
				poppler::PopplerDocument::new_from_data(data, "").unwrap(),
				song,
			)
		};
		let metadata: SongMeta =
			unblock! { serde_json::from_reader(song.by_name("staves.json").unwrap()).unwrap() };
		// TODO clean that up
		futures::future::OptionFuture::<_>::from(metadata.staves.first().map(|line| {
			Staff::new_preview_image(pages.get_page(line.page.into()).unwrap(), line, 0)
		}))
		.await
	}
}

pub struct Library {
	pub songs: HashMap<String, PathBuf>,
}

impl Library {
	pub async fn load_song(&self,
		name: &str,
		image_cache: Rc<RefCell<lru_disk_cache::LruDiskCache>>,
	) -> Song {
		Song::new(self.songs.get(name).unwrap(), image_cache).await
	}
}

#[derive(Debug)]
pub struct Staff {
	rendered: Vec<(f64, cairo::ImageSurface)>, /* It isn't worth using a BTreeMap for this few entries */
	raw: either::Either<gdk_pixbuf::Pixbuf, poppler::PopplerPage>,
	raw_start: (f64, f64),
	raw_end: (f64, f64),
	pub aspect_ratio: f64,
}

impl Staff {
	pub async fn new_from_pdf<'a>(
		page: poppler::PopplerPage,
		line: &Line,
		line_id: usize,
		image_cache: Rc<RefCell<lru_disk_cache::LruDiskCache>>,
		// image_cache: &Rc<RefCell<lru_disk_cache::LruDiskCache>>,
		song_id: impl AsRef<std::ffi::OsStr> + 'a,
		song_version: usize,
	) -> Self {
		let image_cache = &mut *image_cache.borrow_mut();
		// Convert from relative sizes back to pixels
		let line_width = line.get_width() * page.get_size().0 as f64;
		let line_height = line.get_height() * page.get_size().1 as f64;
		let aspect_ratio = line_height / line_width;

		let stuff =
			cairo::ImageSurface::create(cairo::Format::Rgb24, 1200, (1200.0 * aspect_ratio) as i32)
				.unwrap();
		let context = cairo::Context::new(&stuff);

		// let cached = gio::File::new_for_path(format!("./res/{}/cache/{}.png", name, line_id))
		// 	.read_async_future(glib::source::Priority::default())
		// 	.and_then(|stream| gdk_pixbuf::Pixbuf::from_stream_async_future(&stream))
		// 	.await
		// 	.ok();

		let key = {
			let mut key = std::ffi::OsString::new();
			key.push(song_id);
			key.push(song_version.to_string());
			key.push(line_id.to_string());
			key
		};

		if image_cache.contains_key(&key) {
			let cached: gdk_pixbuf::Pixbuf = image_cache.get(&key)
				.map(gio::ReadInputStream::new_seekable)
				// TODO make async again
				// .and_then(|stream| gdk_pixbuf::Pixbuf::from_stream_async_future(&stream).await)
				.map(|stream| gdk_pixbuf::Pixbuf::from_stream(&stream, None::<&gio::Cancellable>).unwrap())
				.unwrap();
			context.set_source_pixbuf(&cached, 0.0, 0.0);
			context.paint();
			stuff.flush();
		} else {
			println!("Rendering small thumbnail");
			let scale = stuff.get_width() as f64 / line_width;
			context.scale(scale, scale);
			context.translate(
				-line.start.0 * page.get_size().0 as f64,
				-line.start.1 * page.get_size().1 as f64,
			);
			context.set_source_rgb(1.0, 1.0, 1.0);
			context.paint();
			page.render(&context);

			stuff.flush();

			// use std::fs::OpenOptions;

			// let mut file = OpenOptions::new()
			// 	.write(true)
			// 	.create(true)
			// 	.open(format!("./res/{}/cache/{}.png", name, line_id))
			// 	.unwrap();
			dbg!(image_cache.path().join(&key));
			image_cache.insert_with(&key, |mut file| {
				stuff.write_to_png(&mut file).unwrap();
				Ok(())
			}).unwrap();
		}

		Staff {
			rendered: vec![(f64::NAN, stuff)],
			raw: either::Right(page),
			raw_start: line.start,
			raw_end: line.end,
			aspect_ratio,
		}
	}

	pub async fn new_preview_image(
		page: poppler::PopplerPage,
		line: &Line,
		line_id: usize,
	) -> gdk_pixbuf::Pixbuf {
		// Convert from relative sizes back to pixels
		let line_width = line.get_width() * page.get_size().0 as f64;
		let line_height = line.get_height() * page.get_size().1 as f64;
		let aspect_ratio = line_height / line_width;

		let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, 400, 100).unwrap();
		let context = cairo::Context::new(&surface);

		let scaleX = surface.get_width() as f64 / line_width;
		// let scaleY = surface.get_height() as f64 / line_height;
		let scale = scaleX;
		context.scale(scale, scale);
		context.translate(
			-line.start.0 * page.get_size().0 as f64,
			-line.start.1 * page.get_size().1 as f64,
		);
		context.set_source_rgb(1.0, 1.0, 1.0);
		context.paint();
		page.render(&context);

		surface.flush();

		gdk::pixbuf_get_from_surface(&surface, 0, 0, surface.get_width(), surface.get_height())
			.unwrap()
	}

	pub fn render(&self, context: &cairo::Context, staff_layout: &StaffLayout) {
		let img = &self.rendered[0].1;
		let scale = staff_layout.width / img.get_width() as f64;

		context.save();
		context.translate(staff_layout.x, staff_layout.y);
		context.scale(scale, scale);

		/* Staff */
		context.set_source_surface(img, 0.0, 0.0);
		context.paint();

		/* Staff number */
		context.save();
		context.set_font_size(20.0);
		context.set_source_rgba(0.0, 0.0, 0.0, 1.0);
		context.move_to(10.0, 16.0);
		context.show_text(&staff_layout.index.to_string());
		context.restore();

		context.restore();
	}
}

pub fn create_progress_bar_dialog(text: &str) -> (gtk::Dialog, gtk::ProgressBar) {
	let progress = gtk::Dialog::new();
	progress.set_modal(true);
	progress.set_skip_taskbar_hint(true);
	progress.set_destroy_with_parent(true);
	progress.set_position(gtk::WindowPosition::CenterOnParent);
	let bar = gtk::ProgressBar::new();
	bar.set_show_text(true);
	bar.set_text(Some(text));
	progress.get_content_area().add(&bar);
	progress.set_title("Loading…");
	progress.set_deletable(false);
	progress.show_all();
	(progress, bar)
}
