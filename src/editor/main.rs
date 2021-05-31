use actix::prelude::*;
use gdk::prelude::*;
use gio::prelude::*;
use glib::clone;
use gtk::prelude::*;
use uuid::Uuid;
use anyhow::Context;

use std::{collections::BTreeMap, rc::Rc};

use dinoscore::{collection::*, *};

mod editor;
use editor::*;

/**
 * Representation of a [`collection::SongFile`] together with its
 * [`SongMeta`](collection::SongMeta) as required by the editor
 */
struct EditorSongFile {
	pages: Vec<(Rc<RawPageImage>, Vec<Staff>)>,

	piece_starts: BTreeMap<StaffIndex, String>,
	section_starts: BTreeMap<StaffIndex, SectionMeta>,

	/// A unique identifier for this song that is stable across file modifications
	song_uuid: Uuid,
	// /// Effectively a random string generated on each save. Useful for caching
	// version_uuid: Uuid,
}

impl EditorSongFile {
	fn new() -> Self {
		Self {
			pages: Vec::new(),
			piece_starts: {
				let mut map = BTreeMap::new();
				map.insert(0.into(), "".into());
				map
			},
			section_starts: {
				let mut map = BTreeMap::new();
				map.insert(0.into(), SectionMeta::default());
				map
			},
			song_uuid: Uuid::new_v4(),
		}
	}

	fn get_staves(&self) -> Vec<Staff> {
		self.pages
			.iter()
			.enumerate()
			.flat_map(|(_page_index, page)| page.1.iter())
			.cloned()
			.collect()
	}

	fn get_pages(&self) -> Vec<Rc<RawPageImage>> {
		self.pages.iter().map(|(page, _)| page).cloned().collect()
	}

	fn count_staves_before(&self, page: PageIndex) -> usize {
		self.pages[0..*page].iter().map(|p| p.1.len()).sum()
	}

	fn shift_items(&mut self, threshold: usize, offset: isize) {
		/* I whish Rust had generic closures or partially applied functions */
		fn mapper<T: Clone>(
			threshold: usize,
			offset: isize,
		) -> impl Fn((&StaffIndex, &mut T)) -> (StaffIndex, T) {
			move |(&index, value)| {
				if *index > threshold {
					(
						StaffIndex((*index as isize + offset) as usize),
						value.clone(),
					)
				} else {
					(index, value.clone())
				}
			}
		}
		/* TODO replace with `drain_filter` once stabilized */
		self.piece_starts = self
			.piece_starts
			.iter_mut()
			.map(mapper(threshold, offset))
			.collect();
		self.section_starts = self
			.section_starts
			.iter_mut()
			.map(mapper(threshold, offset))
			.collect();
	}

	fn add_page(&mut self, page: RawPageImage) {
		self.pages.push((Rc::new(page), vec![]));
	}

	fn remove_page(&mut self, page_index: PageIndex) {
		let (_page, staves) = self.pages.remove(*page_index);
		self.shift_items(
			self.count_staves_before(page_index),
			-(staves.len() as isize),
		);
		self.pages[*page_index..].iter_mut()
			.flat_map(|(page, staves)| staves)
			.for_each(|staff| {
				staff.page -= PageIndex(1);
			});
	}

	fn add_staves(&mut self, page_index: PageIndex, staves: Vec<Staff>) {
		self.shift_items(
			self.count_staves_before(page_index) + staves.len(),
			staves.len() as isize,
		);
		self.pages[*page_index].1.extend(staves);
	}

	/** The `staff` parameter is relative to the page index */
	fn delete_staff(&mut self, page_index: PageIndex, staff: usize) {
		self.shift_items(self.count_staves_before(page_index) + staff, -1);
		self.pages[*page_index].1.remove(staff);
	}

	fn save(&self, file: std::path::PathBuf) -> anyhow::Result<()> {
		let song = SongMeta {
			n_pages: self.pages.len(),
			staves: self.get_staves(),
			piece_starts: self.piece_starts.clone(),
			section_starts: self.section_starts.clone(),
			song_uuid: self.song_uuid,
			version_uuid: uuid::Uuid::new_v4(),
			title: None,
			composer: None,
		};
		use std::ops::Deref;
		let thumbnail = SongFile::generate_thumbnail(
			&song,
			self.pages.iter().map(|(page, _)| page.deref()),
		);
		SongFile::save(
			file,
			song,
			self.pages.iter().map(|(page, _)| page.deref()),
			thumbnail,
			true, // TODO overwrite?!
		)?;
		Ok(())
	}
}

struct AppActor {
	widgets: Rc<AppWidgets>,
	application: gtk::Application,
	editor: actix::Addr<EditorActor>,
	file: EditorSongFile,

	selected_page: Option<PageIndex>,
	/* Relative to the currently selected page */
	selected_staff: Option<usize>,
}

#[derive(woab::WidgetsFromBuilder)]
struct AppWidgets {
	window: gtk::ApplicationWindow,
	menubar: gio::MenuModel,

	pages_preview: gtk::IconView,
	/* Pixbufs preview cache */
	#[widget(name = "store_pages")]
	pages_preview_data: gtk::ListStore,

	piece_start: gtk::CheckButton,
	piece_name: gtk::Entry,
	section_start: gtk::CheckButton,
	section_repetition: gtk::CheckButton,
	section_end: gtk::CheckButton,
}

impl actix::Actor for AppActor {
	type Context = actix::Context<Self>;

	fn started(&mut self, ctx: &mut Self::Context) {
		let application = &self.application;
		let window = &self.widgets.window;

		// window.set_application(Some(&self.application)); // <-- This line segfaults
		window.set_position(gtk::WindowPosition::Center);
		window.add_events(
			gdk::EventMask::STRUCTURE_MASK
				| gdk::EventMask::BUTTON_PRESS_MASK
				| gdk::EventMask::KEY_PRESS_MASK,
		);
		self.application.set_menubar(Some(&self.widgets.menubar));

		let new = gio::SimpleAction::new("new", None);
		woab::route_signal(&new, "activate", "NewDocument", ctx.address()).unwrap();
		application.add_action(&new);
		application.set_accels_for_action("app.new", &["<Primary>N"]);

		let open = gio::SimpleAction::new("open", None);
		woab::route_signal(&open, "activate", "OpenDocument", ctx.address()).unwrap();
		application.add_action(&open);
		application.set_accels_for_action("app.open", &["<Primary>O"]);

		let save = gio::SimpleAction::new("save", None);
		woab::route_signal(&save, "activate", "SaveDocument", ctx.address()).unwrap();
		application.add_action(&save);
		application.set_accels_for_action("app.save", &["<Primary>S"]);

		let quit = gio::SimpleAction::new("quit", None);
		quit.connect_activate(
			clone!(@weak application => @default-panic, move |_action, _parameter| {
				log::debug!("Quit for real");
				application.quit();
			}),
		);
		application.add_action(&quit);
		application.set_accels_for_action("app.quit", &["<Primary>Q"]);
		window.connect_destroy(clone!(@weak application => @default-panic, move |_| {
			log::debug!("Destroy quit");
			application.quit();
		}));

		woab::route_signal(
			&self.widgets.pages_preview,
			"selection-changed",
			"SelectPage",
			ctx.address(),
		)
		.unwrap();

		window.show_all();
	}

	fn stopped(&mut self, _ctx: &mut Self::Context) {
		log::debug!("Actor Quit");
		// gtk::main_quit();
	}
}

impl AppActor {
	fn new(
		widgets: AppWidgets,
		application: gtk::Application,
		editor: actix::Addr<EditorActor>,
	) -> Self {
		widgets.window.set_application(Some(&application));
		let mut this = Self {
			widgets: Rc::new(widgets),
			application,
			editor,
			file: EditorSongFile::new(),
			selected_page: None,
			selected_staff: None,
		};
		/* Enforce some invariants */
		this.unload_and_clear();
		this
	}

	fn add_pages(&mut self, ctx: &mut <Self as actix::Actor>::Context) {
		let addr = ctx.address();

		glib::MainContext::default().spawn_local_with_priority(
			glib::source::PRIORITY_DEFAULT_IDLE,
			async move {
				let filter = gtk::FileFilter::new();
				filter.add_pixbuf_formats();
				filter.add_mime_type("application/pdf");
				let choose = gtk::FileChooserNativeBuilder::new()
					.title("Select images or PDFs to load")
					.action(gtk::FileChooserAction::Open)
					.select_multiple(true)
					.filter(&filter)
					.build();

				if choose.run() == gtk::ResponseType::Accept {
					let (progress_dialog, progress) =
						dinoscore::create_progress_bar_dialog("Loading pages …");
					let total_work = choose.get_files().len();

					for (i, file) in choose.get_files().iter().enumerate() {
						let path = file.get_path().unwrap();

						let pages = blocking::unblock(move || {
							let raw = std::fs::read(path.as_path()).unwrap();
							let extension =
								path.as_path().extension().and_then(std::ffi::OsStr::to_str);
							let pages = if let Some("pdf") = extension {
								page_image::explode_pdf_full(&raw, |raw, page| {
									RawPageImage::Vector { page, raw }
								})
								.unwrap()
							} else {
								vec![RawPageImage::Raster {
									image: gdk_pixbuf::Pixbuf::from_file(&path).unwrap(),
									raw,
									extension: extension
										.expect("Image files must have an extension")
										.to_string(),
								}]
							};
							unsafe { unsafe_force::Send::new(pages) }
						})
						.await;

						progress.set_fraction((i + 1) as f64 / total_work as f64);

						addr.try_send(RunLater::new(move |this: &mut Self| {
							for page in unsafe { pages.unwrap() } {
								this.add_page(page);
							}
						}))
						.unwrap();
					}
					async_std::task::sleep(std::time::Duration::from_millis(350)).await;
					progress_dialog.emit_close();
				}
			},
		);
	}

	fn add_pages2(&mut self) {
		// 				let selected_items = pages_preview.get_selected_items();
		// 				selected_items.iter()
		// 					.map(|selected| selected.get_indices()[0] as usize)
		// 					.for_each(|i| {
		// 						let bars_inner = vec![Staff {
		// 							left: 0.0, right: 1.0, top: 0.0, bottom: 1.0,
		// 						}];
		// 						state.borrow_mut().add_staves(i.into(), bars_inner);
		// 					});

		// let filter = gtk::FileFilter::new();
		// filter.add_pixbuf_formats();
		// let choose = gtk::FileChooserNativeBuilder::new()
		// 	.title("Select images to load")
		// 	.action(gtk::FileChooserAction::Open)
		// 	.select_multiple(true)
		// 	.filter(&filter)
		// 	.build();
		// if choose.run() == gtk::ResponseType::Accept {
		// 	for file in choose.get_files() {
		// 		let path = file.get_path().unwrap();
		// 		let image = opencv::imgcodecs::imread(&path.to_str().unwrap(), 0).unwrap();

		// 		let mut image_binarized = opencv::core::Mat::default().unwrap();
		// 		opencv::imgproc::adaptive_threshold(&image, &mut image_binarized, 255.0,
		// 			opencv::imgproc::AdaptiveThresholdTypes::ADAPTIVE_THRESH_MEAN_C as i32,
		// 			opencv::imgproc::ThresholdTypes::THRESH_BINARY as i32,
		// 			101, 30.0
		// 		).unwrap();

		// 		let mut image_binarized_median = opencv::core::Mat::default().unwrap();
		// 		opencv::imgproc::median_blur(&image_binarized, &mut image_binarized_median, 3).unwrap();

		// 		dbg!(opencv::imgcodecs::imwrite("./tmp.png", &image_binarized_median, &opencv::core::Vector::new()).unwrap());
		// 		/* The easiest way to convert Mat to Pixbuf is to write it to a PNG buffer */
		// 		let mut png = opencv::core::Vector::new();
		// 		dbg!(opencv::imgcodecs::imencode(
		// 			".png",
		// 			&image_binarized_median,
		// 			&mut png,
		// 			&opencv::core::Vector::new(),
		// 		).unwrap());
		// 		let pixbuf = gdk_pixbuf::Pixbuf::from_stream(
		// 			/* How many type conversion layers will we pile today? */
		// 			&gio::MemoryInputStream::from_bytes(&glib::Bytes::from(&png.to_vec())),
		// 			Option::<&gio::Cancellable>::None,
		// 		).unwrap();
		// 		let pdf = pixbuf_to_pdf(&pixbuf);
		// 		for page in 0..pdf.get_n_pages() {
		// 			let page = pdf.get_page(page).unwrap();
		// 			state.borrow_mut().add_page(page);
		// 		}
		// 	}
		// }
	}

	fn add_page(&mut self, page: RawPageImage) {
		let pixbuf = page.render_to_thumbnail(400);
		self.file.add_page(page);

		self.widgets.pages_preview_data.set(
			&self.widgets.pages_preview_data.append(),
			&[0],
			&[&pixbuf],
		);
	}

	fn remove_page(&mut self, page: PageIndex) {
		if self.selected_page == Some(page) {
			self.select_page(None);
		}
		self.file.remove_page(page);

		clone!(@strong self.widgets as widgets => move || woab::spawn_outside(async move {
			widgets.pages_preview_data.remove(
				&widgets.pages_preview_data.get_iter(&gtk::TreePath::from_indicesv(&[*page as i32])).unwrap()
			);
		}))();
	}

	fn add_staves(&mut self, page_index: PageIndex, staves: Vec<Staff>) {
		self.file.add_staves(page_index, staves);

		if self.selected_page == Some(page_index) {
			self.editor
				.try_send(EditorSignal2::LoadPage(
					self.selected_page
						.map(|page_index| {
							let (page, bars) = self.file.pages[*page_index].clone();
							(page, bars, self.file.count_staves_before(page_index))
						})
						.into(),
				))
				.unwrap();
		}
	}

	fn select_page(&mut self, selected_page: Option<PageIndex>) {
		self.selected_page = selected_page;
		self.editor
			.try_send(EditorSignal2::LoadPage(
				self.selected_page
					.map(|page_index| {
						let (page, bars) = self.file.pages[page_index.0].clone();
						(page, bars, self.file.count_staves_before(page_index))
					})
					.into(),
			))
			.unwrap();
	}

	pub fn autodetect(&mut self, ctx: &mut <Self as actix::Actor>::Context) {
		let selected_items = self.widgets.pages_preview.get_selected_items();

		let widgets = self.widgets.clone();
		let pdf_pages = self.file.get_pages();
		let address = ctx.address();

		ctx.spawn(actix::fut::wrap_future(async move {
			let (progress_dialog, progress) =
				dinoscore::create_progress_bar_dialog("Detecting staves …");
			let total_work = selected_items.len();
			async_std::task::yield_now().await;

			for (i, page) in selected_items
				.into_iter()
				.map(|selected| selected.get_indices()[0] as usize)
				.enumerate()
			{
				let data = widgets
					.pages_preview_data
					.get_value(
						&widgets
							.pages_preview_data
							.iter_nth_child(None, page as i32)
							.unwrap(),
						0,
					)
					.downcast::<gdk_pixbuf::Pixbuf>()
					.unwrap()
					.get()
					.unwrap();
				let pdf_page = &pdf_pages[page];
				let width = pdf_page.get_width() as f64;
				let height = pdf_page.get_height() as f64;

				let data = unsafe { unsafe_force::Send::new(data) };
				let (page, bars_inner) = blocking::unblock(move || {
					log::info!("Autodetecting {} ({}/{})", page, i, total_work);
					let page = PageIndex(page);
					let bars_inner: Vec<Staff> =
						recognition::recognize_staves(&unsafe { data.unwrap() })
							.iter()
							.cloned()
							.map(|staff| staff.into_staff(page, width, height))
							.collect();
					(page, bars_inner)
				})
				.await;
				progress.set_fraction((i + 1) as f64 / total_work as f64);

				// actix::fut::ready(()).map(|(), this: &mut Self, _ctx|  {
				// });
				address
					.try_send(RunLater::new(move |this: &mut Self| {
						this.add_staves(page, bars_inner);
					}))
					.unwrap();
			}

			async_std::task::sleep(std::time::Duration::from_millis(350)).await;
			progress_dialog.emit_close();
			async_std::task::yield_now().await;
			log::info!("Autodetected");
		}));
	}

	fn update_selection(
		&mut self,
		selected_staff: Option<usize>,
		ctx: &mut <Self as actix::Actor>::Context,
	) {
		self.selected_staff = selected_staff;

		self.update_bottom_widgets(ctx);
	}

	fn delete_selected_staff(&mut self, ctx: &mut <Self as actix::Actor>::Context) {
		if self.selected_staff.is_none() {
			return;
		}
		let staff = self.selected_staff.take().unwrap();

		self.update_bottom_widgets(ctx);

		self.file.delete_staff(self.selected_page.unwrap(), staff);

		self.editor
			.try_send(EditorSignal2::LoadPage(
				self.selected_page
					.map(|page_index| {
						let (page, bars) = self.file.pages[*page_index].clone();
						(page, bars, self.file.count_staves_before(page_index))
					})
					.into(),
			))
			.unwrap();
	}

	fn update_part_name(&mut self, new_name: &str, ctx: &mut <Self as actix::Actor>::Context) {
		if self.selected_page.is_none() || self.selected_staff.is_none() {
			return;
		}
		let index = StaffIndex(
			self.file.count_staves_before(self.selected_page.unwrap())
				+ self.selected_staff.unwrap(),
		);
		let name = self
			.file
			.piece_starts
			.get_mut(&index)
			.expect("You shouldn't be able to set the name on non part starts");
		*name = new_name.to_string();

		self.update_bottom_widgets(ctx);
	}

	fn update_section_start(&mut self, selected: bool, ctx: &mut <Self as actix::Actor>::Context) {
		if self.selected_page.is_none() || self.selected_staff.is_none() {
			return;
		}
		let index = StaffIndex(
			self.file.count_staves_before(self.selected_page.unwrap())
				+ self.selected_staff.unwrap(),
		);
		if selected {
			self.file
				.section_starts
				.entry(index)
				.or_insert_with(SectionMeta::default);
		} else {
			self.file.section_starts.remove(&index);
		}

		self.update_bottom_widgets(ctx);
	}

	fn update_section_repetition(
		&mut self,
		selected: bool,
		ctx: &mut <Self as actix::Actor>::Context,
	) {
		if self.selected_page.is_none() || self.selected_staff.is_none() {
			return;
		}
		let index = StaffIndex(
			self.file.count_staves_before(self.selected_page.unwrap())
				+ self.selected_staff.unwrap(),
		);
		self.file
			.section_starts
			.get_mut(&index)
			.expect("You shouldn't be able to click this if there's no section start")
			.is_repetition = selected;

		self.update_bottom_widgets(ctx);
	}

	fn update_section_end(&mut self, selected: bool, ctx: &mut <Self as actix::Actor>::Context) {
		if self.selected_page.is_none() || self.selected_staff.is_none() {
			return;
		}
		let index = StaffIndex(
			self.file.count_staves_before(self.selected_page.unwrap())
				+ self.selected_staff.unwrap(),
		);
		self.file
			.section_starts
			.get_mut(&index)
			.expect("You shouldn't be able to click this if there's no section start")
			.section_end = selected;

		self.update_bottom_widgets(ctx);
	}

	fn update_part_start(&mut self, selected: bool, ctx: &mut <Self as actix::Actor>::Context) {
		if self.selected_page.is_none() || self.selected_staff.is_none() {
			return;
		}
		let index = StaffIndex(
			self.file.count_staves_before(self.selected_page.unwrap())
				+ self.selected_staff.unwrap(),
		);
		if selected {
			self.file.piece_starts.entry(index).or_insert_with(|| "".into());
			/* When a piece starts, a section must start as well */
			self.file
				.section_starts
				.entry(index)
				.or_insert_with(SectionMeta::default);
		} else {
			self.file.piece_starts.remove(&index);
		}

		self.update_bottom_widgets(ctx);
	}

	fn update_bottom_widgets(&mut self, ctx: &mut <Self as actix::Actor>::Context) {
		let index: Option<usize> = self
			.selected_page
			.map(|page| self.file.count_staves_before(page))
			.and_then(|staff| self.selected_staff.map(|s| staff + s));

		/* Set the selection */
		let piece_start_active = index
			.and_then(|i| self.file.piece_starts.get(&StaffIndex(i)))
			.is_some();

		let piece_name: String = index
			.and_then(|i: usize| self.file.piece_starts.get(&StaffIndex(i)))
			.cloned()
			.unwrap_or_default();
		let section_start: Option<&SectionMeta> =
			index.and_then(|i| self.file.section_starts.get(&i.into()));
		let section_has_repetition = section_start
			.map(|meta| meta.is_repetition)
			.unwrap_or(false);
		let has_section_start = section_start.is_some();
		let has_section_end = section_start.map(|meta| meta.section_end).unwrap_or(false);

		/* Set the selected_staff to None to implicitly inhibit events */
		let selected_staff_backup = self.selected_staff.take();

		/* In Swing/JavaFX, "active"=>"selected", "sensitive"=>"enabled"/"not disabled" */

		let fut = clone!(@strong self.widgets as widgets => move || woab::outside(async move {
			/* Disable the check box for the first item (it's force selected there) */
			let piece_start_sensitive = index.map(|i| i > 0).unwrap_or(false);
			widgets.piece_start.set_sensitive(piece_start_sensitive);
			widgets.piece_start.set_active(piece_start_active);

			/* You can only enter a name on piece starts */
			widgets.piece_name.set_sensitive(piece_start_active);
			widgets.piece_name.set_text(&piece_name);
			/* When a piece starts, a section must start as well, so it can't be edited */
			widgets.section_start
				.set_sensitive(!piece_start_active && piece_start_sensitive);
			widgets.section_start.set_active(has_section_start);

			widgets.section_repetition.set_sensitive(has_section_start);
			widgets.section_repetition.set_active(section_has_repetition);
			widgets.section_end.set_sensitive(has_section_start);
			widgets.section_end.set_active(has_section_end);
		}))();
		let fut = fut.into_actor(self);
		let fut = fut.map(move |result, this: &mut Self, _ctx| {
			result.unwrap();
			this.selected_staff = selected_staff_backup;
		});
		ctx.spawn(fut);
	}

	fn unload_and_clear(&mut self) {
		// self.pages_preview.block_signal(&self.pages_preview_callback);
		self.select_page(None);
		self.file = EditorSongFile::new();
		self.widgets.pages_preview_data.clear();
		// self.pages_preview.unblock_signal(&self.pages_preview_callback);
	}

	fn load_with_dialog(&mut self) {
		let filter = gtk::FileFilter::new();
		filter.add_mime_type("application/zip");
		let choose = gtk::FileChooserNativeBuilder::new()
			.title("File to load")
			.action(gtk::FileChooserAction::Open)
			.select_multiple(false)
			.filter(&filter)
			.build();
		if choose.run() == gtk::ResponseType::Accept {
			if let Some(file) = choose.get_file() {
				let path = file.get_path().unwrap();
				let mut song = SongFile::new(path).unwrap();
				self.load(song.load_sheets_raw().unwrap(), song.index);
			}
		}
	}

	fn load(&mut self, pages: Vec<RawPageImage>, song: SongMeta) {
		self.unload_and_clear();
		for (index, page) in pages.into_iter().enumerate() {
			self.add_page(page);
			self.add_staves(
				PageIndex(index),
				song.staves
					.iter()
					.filter(|line| line.page == index.into())
					.cloned()
					.collect::<Vec<_>>(),
			);
		}
		self.file.piece_starts = song.piece_starts;
		self.file.section_starts = song.section_starts;
	}

	fn save_with_ui(&self, ctx: &mut <Self as actix::Actor>::Context) {
		log::info!("Saving staves");

		let filter = gtk::FileFilter::new();
		filter.add_mime_type("application/zip");
		let choose = gtk::FileChooserNativeBuilder::new()
			.title("Save song")
			.action(gtk::FileChooserAction::Save)
			.select_multiple(false)
			.do_overwrite_confirmation(true)
			.filter(&filter)
			.build();

		use actix::fut::*;
		ctx.spawn(
			woab::outside(async move { (choose.clone(), choose.run()) })
				.into_actor(self)
				.map(move |result, this, _ctx| {
					let (choose, result) = result.unwrap();
					if result == gtk::ResponseType::Accept {
						if let Some(file) = choose.get_file() {
							this.file.save(file.get_path().unwrap()).unwrap();
						}
					}
				}),
		);
	}
}

#[derive(actix::Message)]
#[rtype(result = "()")]
struct RunLater<T: Actor, F: FnOnce(&mut T)>(unsafe_force::Send<(F, std::marker::PhantomData<T>)>);

impl<T: Actor, F: FnOnce(&mut T)> RunLater<T, F> {
	fn new(closure: F) -> Self {
		Self(unsafe { unsafe_force::Send::new((closure, std::marker::PhantomData)) })
	}
}

impl<F: FnOnce(&mut AppActor)> actix::Handler<RunLater<AppActor, F>> for AppActor {
	type Result = ();

	fn handle(&mut self, message: RunLater<AppActor, F>, _ctx: &mut Self::Context) -> Self::Result {
		unsafe { message.0.unwrap() }.0(self);
	}
}

#[derive(actix::Message)]
#[rtype(result = "()")]
struct StaffSelected(Option<usize>);

impl actix::Handler<StaffSelected> for AppActor {
	type Result = ();

	fn handle(&mut self, selected_staff: StaffSelected, ctx: &mut Self::Context) {
		let selected_staff = selected_staff.0;
		self.update_selection(selected_staff, ctx);
	}
}

#[derive(actix::Message)]
#[rtype(result = "()")]
struct DeleteSelectedStaff;

impl actix::Handler<DeleteSelectedStaff> for AppActor {
	type Result = ();

	fn handle(&mut self, _: DeleteSelectedStaff, ctx: &mut Self::Context) {
		self.delete_selected_staff(ctx);
	}
}

impl actix::Handler<woab::Signal> for AppActor {
	type Result = woab::SignalResult;

	fn handle(&mut self, signal: woab::Signal, ctx: &mut Self::Context) -> woab::SignalResult {
		log::debug!("Signal: {:?}", signal.name());
		signal!(match (signal) {
			/* Menu */
			"OpenDocument" => {self.load_with_dialog()},
			"NewDocument" => {self.unload_and_clear()},
			"SaveDocument" => {self.save_with_ui(ctx)},
			"PieceStartUpdate" => |piece_start = gtk::CheckButton| {
				self.update_part_start(piece_start.get_active(), ctx)
			},
			/* Tool bar */
			"add_pages" => {
				self.add_pages(ctx)
			},
			"add_pages2" => { /* TODO */ },
			"SelectPage" => {
				let selected_items = self.widgets.pages_preview.get_selected_items();
				self.select_page(match selected_items.len() {
					0 => None,
					1 => Some(PageIndex(selected_items[0].get_indices()[0] as usize)),
					_ => None,
				});
			},
			"autodetect" => {
				self.autodetect(ctx)
			},
			/* Side bar */
			"key_press" => |pages_preview = gtk::IconView, event = gdk::Event| {
				let event: gdk::EventKey = event.downcast().unwrap();

				if event.get_keyval() == gdk::keys::constants::Delete
				|| event.get_keyval() == gdk::keys::constants::KP_Delete {
					let selected_items = pages_preview.get_selected_items();
					selected_items.iter()
						.map(|selected| selected.get_indices()[0] as usize)
						.for_each(|i| {
							self.remove_page(PageIndex(i));
						});
					return Ok(Some(Inhibit(true)))
				} else {
					return Ok(Some(Inhibit(false)))
				}
			},
			/* Editor */
			"piece_start_update" => {
				first_arg!(signal, piece_start: gtk::CheckButton);
				self.update_part_start(piece_start.get_active(), ctx)
			},
			"piece_name_update" => {
				first_arg!(signal, piece_name: gtk::Entry);
				self.update_part_name(&piece_name.get_text(), ctx)
			},
			"section_start_update" => {
				first_arg!(signal, section_start: gtk::CheckButton);
				self.update_section_start(section_start.get_active(), ctx)
			},
			"section_end_update" => {
				first_arg!(signal, section_end: gtk::CheckButton);
				self.update_section_end(section_end.get_active(), ctx)
			},
			"section_repetition_update" => {
				first_arg!(signal, section_repetition: gtk::CheckButton);
				self.update_section_repetition(section_repetition.get_active(), ctx)
			},
			_ => {unreachable!()},
		});
		Ok(None)
	}
}

#[allow(clippy::all)]
fn main() -> anyhow::Result<()> {
	simple_logger::SimpleLogger::new()
		.with_level(log::LevelFilter::Trace)
		.init()
		.context("Failed to initialize logger")?;
	let orig_hook = std::panic::take_hook();
	std::panic::set_hook(Box::new(move |panic_info| {
		// invoke the default handler and exit the process
		orig_hook(panic_info);
		std::process::exit(1);
	}));

	let application = gtk::Application::new(
		Some("de.piegames.dinoscore.editor"),
		gio::ApplicationFlags::NON_UNIQUE,
	)
	.context("Initialization failed")?;

	application.connect_startup(|_application| {
		/* This is required so that builder can find this type. See gobject_sys::g_type_ensure */
		let _ = gio::ThemedIcon::static_type();
		libhandy::init();
		woab::run_actix_inside_gtk_event_loop().unwrap(); // <===== IMPORTANT!!!
		log::info!("Woab started");
	});

	application.connect_activate(move |application| {
		let builder = gtk::Builder::from_file("res/editor.glade");
		let builder = woab::BuilderConnector::from(builder);

		woab::block_on(async {
			AppActor::create(clone!(@weak application => @default-panic, move |ctx1| {
				let widgets = builder.widgets().unwrap();
				let editor = EditorActor::create(|ctx2| {
					let builder = builder.connect_to(woab::NamespacedSignalRouter::default()
						.route(ctx1.address())
						.route(ctx2.address())
					);
					EditorActor::new(ctx1.address(), builder.widgets().unwrap())
				});
				AppActor::new(widgets, application, editor)
			}));
		});

		log::info!("Application started");
	});

	application.run(&[]);
	log::info!("Thanks for using DiNoScore.");
	Ok(())
}
