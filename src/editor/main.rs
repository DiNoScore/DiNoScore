use std::collections::BTreeMap;
use actix::prelude::*;
use gdk::prelude::*;
use gio::prelude::*;
use glib::clone;
use gtk::prelude::*;
use std::rc::Rc;

use dinoscore::collection::*;
use dinoscore::recognition;

mod editor;
use editor::*;

// impl EditorView {
// 	fn new(builder: &gtk::Builder) -> Rc<RefCell<Self>> {

// 		pages_preview.connect_key_press_event(clone!(@strong state =>
// 			move |pages_preview, event| {
// 				if event.get_keyval() == gdk::keys::constants::Delete
// 				|| event.get_keyval() == gdk::keys::constants::KP_Delete {
// 					let state = &mut *state.borrow_mut();
// 					let selected_items = pages_preview.get_selected_items();
// 					selected_items.iter()
// 						.map(|selected| selected.get_indices()[0] as usize)
// 						.for_each(|i| {
// 							state.remove_page(PageIndex(i));
// 						});
// 				}
// 				gtk::Inhibit::default()
// 			}
// 		));


struct AppActor {
	widgets: AppWidgets,
	application: gtk::Application,
	editor: actix::Addr<EditorActor>,

// 	update_ui: glib::Sender<EditorViewUpdateEvent>,
	pages: Vec<(Rc<poppler::PopplerPage>, Vec<Staff>)>,
// 	/* Sections */
	piece_starts: BTreeMap<StaffIndex, Option<String>>,
	section_starts: BTreeMap<StaffIndex, SectionMeta>,
	selected_page: Option<PageIndex>,
	selected_staff: Option<usize>,
	// state: Rc<RefCell<EditorState>>,
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
		let connector = AppSignal::connector().route_to::<Self>(ctx);
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
		connector.connect(&new, "activate", "NewDocument").unwrap();
		application.add_action(&new);
		application.set_accels_for_action("app.new", &["<Primary>N"]);
	
		let open = gio::SimpleAction::new("open", None);
		connector.connect(&open, "activate", "OpenDocument").unwrap();
		application.add_action(&open);
		application.set_accels_for_action("app.open", &["<Primary>O"]);
	
		let save = gio::SimpleAction::new("save", None);
		connector.connect(&save, "activate", "SaveDocument").unwrap();
		application.add_action(&save);
		application.set_accels_for_action("app.save", &["<Primary>S"]);

		let quit = gio::SimpleAction::new("quit", None);
		quit.connect_activate(
			clone!(@weak application => @default-panic, move |_action, _parameter| {
				println!("Quit for real");
				application.quit();
			}),
		);
		application.add_action(&quit);
		application.set_accels_for_action("app.quit", &["<Primary>Q"]);
		window.connect_destroy(clone!(@weak application => @default-panic, move |_| {
			println!("Destroy quit");
			application.quit();
		}));

		connector.connect(&self.widgets.pages_preview, "selection-changed", "SelectPage").unwrap();

		window.show_all();
	}

	fn stopped(&mut self, _ctx: &mut Self::Context) {
		println!("Actor Quit");
		// gtk::main_quit();
	}
}

impl AppActor {
	fn new(widgets: AppWidgets, application: gtk::Application, editor: actix::Addr<EditorActor>) -> Self {
		widgets.window.set_application(Some(&application));
		Self {
			widgets,
			application,
			editor,
			pages: Vec::new(),
			selected_page: None,
			selected_staff: None,
			piece_starts: BTreeMap::new(),
			section_starts: BTreeMap::new(),
		}
	}

	fn add_pages(&mut self) {
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
			for file in choose.get_files() {
				let path = file.get_path().unwrap();
				let pdf = if let Some("pdf") = path.as_path().extension().and_then(std::ffi::OsStr::to_str) {
					poppler::PopplerDocument::new_from_file(path, "").unwrap()
				} else {
					pixbuf_to_pdf(&gdk_pixbuf::Pixbuf::from_file(&path).unwrap())
				};
				for page in 0..pdf.get_n_pages() {
					let page = pdf.get_page(page).unwrap();
					self.add_page(page);
				}
			}
		}
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

	fn add_page(&mut self, page: poppler::PopplerPage) {
		let pixbuf = pdf_to_pixbuf(&page, 400);
		self.pages.push((Rc::new(page), vec![]));

		self.widgets.pages_preview_data
			.set(&self.widgets.pages_preview_data.append(), &[0], &[&pixbuf]);
	}

	fn add_staves(&mut self, page_index: PageIndex, staves: Vec<Staff>) {
		self.pages[*page_index].1.extend(staves);
		if self.selected_page == Some(page_index) {
			self.editor.try_send(EditorSignal2::LoadPage(unsafe_send_sync::UnsafeSend::new(
				self.selected_page.map(|page_index| {
					let (page, bars) = self.pages[*page_index].clone();
					(page, bars, self.count_staves_before(page_index))
				})
			))).unwrap();
		}
	}

	fn select_page(&mut self, selected_page: Option<PageIndex>) {
		self.selected_page = selected_page;
		self.editor.try_send(EditorSignal2::LoadPage(unsafe_send_sync::UnsafeSend::new(
			self.selected_page.map(|page_index| {
				let (page, bars) = self.pages[page_index.0].clone();
				(page, bars, self.count_staves_before(page_index))
			})
		))).unwrap();
	}

	pub fn autodetect(&mut self, ctx: &mut <Self as actix::Actor>::Context) {
		let selected_items = self.widgets.pages_preview.get_selected_items();
		use actix::prelude::*;
		let (progress_dialog, progress) = dinoscore::create_progress_bar_dialog("Detecting staves …");
		let total_work = selected_items.len();
		ctx.wait({
			actix::fut::wrap_stream::<_, Self>(futures::stream::iter(
				selected_items.into_iter()
				.map(|selected| selected.get_indices()[0] as usize)
				.enumerate()
				/* Need to manually move/clone out all GTK objects :( */
				.map(move |(i, page)| (i, page, progress.clone()))
			))
			.then(move |(i, page, progress), this, _ctx| {
				let data = this.widgets.pages_preview_data.get_value(
					&this.widgets.pages_preview_data.iter_nth_child(None, page as i32).unwrap(),
					0,
				).downcast::<gdk_pixbuf::Pixbuf>().unwrap().get().unwrap();
				let pdf_page = &this.pages[page].0;
				let width = pdf_page.get_size().0 as f64;
				let height = pdf_page.get_size().1 as f64;

				actix::fut::wrap_future(Box::pin(async move {
					println!("Autodetecting {} ({}/{})", page, i, total_work);
					let page = PageIndex(page);
					let bars_inner: Vec<Staff> = recognition::recognize_staves(&data)
						.iter()
						.cloned()
						.map(|staff| staff.into_staff(page, width, height))
						.collect();
					println!("G");
					progress.set_fraction((i+1) as f64 / total_work as f64);
					(page, bars_inner)
				}))
			})
			.map(move |(page, bars_inner), this, _ctx| {
			// .map(move |_, this, _ctx| {
				println!("H");
				this.add_staves(page, bars_inner);
			})
			.fold((), move |(), _, _this, _ctx| {
				println!("I");
				actix::fut::ready(())
			})
			.then(move |(), _, _| actix::fut::wrap_future(async move {
				println!("J");
				async_std::task::sleep(std::time::Duration::from_millis(350)).await;
				progress_dialog.emit_close();
			}))
		});
		println!("Autodetected");
	}

	fn get_staves(&self) -> Vec<Staff> {
		self.pages
			.iter()
			.enumerate()
			.flat_map(|(page_index, page)| page.1.iter())
			.cloned()
			.collect()
	}

	fn count_staves_before(&self, page: PageIndex) -> usize {
		self.pages[0..*page]
			.iter()
			.map(|p| p.1.len())
			.sum()
	}

	fn update_selection(&mut self, selected_staff: Option<usize>) {
		self.selected_staff = selected_staff;
		let index = self.selected_page
			.map(|page| self.count_staves_before(page))
			.and_then(|staff| {
				selected_staff
					.map(|s| staff + s)
			});

		/* In Swing/JavaFX, "active"=>"selected", "sensitive"=>"enabled"/"not disabled" */

		/* Disable the check box for the first item (it's force selected there) */
		let piece_start_sensitive = index.map(|i| i > 0).unwrap_or(false);
		self.widgets.piece_start.set_sensitive(piece_start_sensitive);
		/* Set the selection */
		let piece_start_active = index
			.map(|i| self.piece_starts.get(&StaffIndex(i)))
			.flatten()
			.is_some();
		self.widgets.piece_start.set_active(piece_start_active);

		/* You can only enter a name on piece starts */
		self.widgets.piece_name.set_sensitive(piece_start_active);
		self.widgets.piece_name.set_text(
			index
				.map(|i| self.piece_starts.get(&StaffIndex(i)))
				.flatten()
				.map(Option::as_ref)
				.flatten()
				.map_or("", String::as_str),
		);
		/* When a piece starts, a section must start as well, so it can't be edited */
		self.widgets.section_start
			.set_sensitive(!piece_start_active && piece_start_sensitive);
		let section_start = index.and_then(|i| self.section_starts.get(&i.into()));
		self.widgets.section_start.set_active(section_start.is_some());

		self.widgets.section_repetition
			.set_sensitive(section_start.is_some());
		self.widgets.section_repetition.set_active(
			section_start
				.map(|meta| meta.is_repetition)
				.unwrap_or(false),
		);
		self.widgets.section_end.set_sensitive(section_start.is_some());
		self.widgets.section_end
			.set_active(section_start.map(|meta| meta.section_end).unwrap_or(false));
	}

	fn update_part_name(&mut self, new_name: &str) {
		let index = self
			.selected_page
			.map(|page| self.count_staves_before(page))
			.and_then(|staff| {
				self.selected_staff
					.map(|s| staff + s)
			})
			.expect("You shouldn't be able to click this with nothing selected");
		let name = self
			.piece_starts
			.get_mut(&index.into())
			.expect("You shouldn't be able to set the name on non part starts");
		*name = Some(new_name.to_string());
	}

	fn update_section_start(&mut self, selected: bool) {
		let index = self
			.selected_page
			.map(|page| self.count_staves_before(page))
			.and_then(|staff| {
				self.selected_staff
					.map(|s| staff + s)
			})
			.expect("You shouldn't be able to click this with nothing selected");
		let index = StaffIndex(index);
		if selected {
			self.section_starts
				.entry(index)
				.or_insert_with(SectionMeta::default);
		} else {
			self.section_starts.remove(&index);
		}
	}

	fn update_section_repetition(&mut self, selected: bool) {
		let index = self
			.selected_page
			.map(|page| self.count_staves_before(page))
			.and_then(|staff| {
				self.selected_staff
					.map(|s| staff + s)
			})
			.expect("You shouldn't be able to click this with nothing selected");
		let index = StaffIndex(index);
		self.section_starts
			.get_mut(&index)
			.expect("You shouldn't be able to click this if there's no section start")
			.is_repetition = selected;
	}

	fn update_section_end(&mut self, selected: bool) {
		let index = self
			.selected_page
			.map(|page| self.count_staves_before(page))
			.and_then(|staff| {
				self.selected_staff
					.map(|s| staff + s)
			})
			.expect("You shouldn't be able to click this with nothing selected");
		let index = StaffIndex(index);
		self.section_starts
			.get_mut(&index)
			.expect("You shouldn't be able to click this if there's no section start")
			.section_end = selected;
	}

	fn update_part_start(&mut self, selected: bool) {
		let index = self
			.selected_page
			.map(|page| self.count_staves_before(page))
			.and_then(|staff| {
				self.selected_staff
					.map(|s| staff + s)
			})
			.expect("You shouldn't be able to click this with nothing selected");
		let index = StaffIndex(index);
		if selected {
			self.piece_starts.entry(index).or_insert(None);
			/* When a piece starts, a section must start as well */
			self.section_starts
				.entry(index)
				.or_insert_with(SectionMeta::default);
		} else {
			self.piece_starts.remove(&index);
		}
	}





	fn unload_and_clear(&mut self) {
		// self.pages_preview.block_signal(&self.pages_preview_callback);
		self.select_page(None);
		self.pages.clear();
		self.widgets.pages_preview_data.clear();
		self.piece_starts.clear();
		self.piece_starts.insert(0.into(), None);
		self.section_starts.clear();
		self.section_starts.insert(0.into(), SectionMeta::default());
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
				let mut song = futures::executor::block_on(SongFile::new(path));
				self.load(song.load_sheet().into_inner(), song.index);
			}
		}
	}

	fn load(&mut self, pages: poppler::PopplerDocument, song: SongMeta) {
		self.unload_and_clear();
		for index in 0..pages.get_n_pages() {
			self.add_page(pages.get_page(index).unwrap());
			self.add_staves(
				PageIndex(index),
				song.staves
					.iter()
					.filter(|line| line.page == index.into())
					.cloned()
					.collect::<Vec<_>>()
			);
		}
		self.piece_starts = song.piece_starts;
		self.section_starts = song.section_starts;
	}

	fn save_with_ui(&mut self) {
		println!("Saving staves");

		let filter = gtk::FileFilter::new();
		filter.add_mime_type("application/zip");
		let choose = gtk::FileChooserNativeBuilder::new()
			.title("Save song")
			.action(gtk::FileChooserAction::Save)
			.select_multiple(false)
			.do_overwrite_confirmation(true)
			.filter(&filter)
			.build();
		if choose.run() == gtk::ResponseType::Accept {
			if let Some(file) = choose.get_file() {
				let song = SongMeta {
					staves: self.get_staves(),
					piece_starts: self.piece_starts.clone(),
					section_starts: self.section_starts.clone(),
					song_uuid: uuid::Uuid::new_v4(), // TODO take that one from somewhere
					version_uuid: uuid::Uuid::new_v4(),
					title: None,
					composer: None,
				};
				let iter_pages = || self.pages.iter()
					.map(|page| &*page.0)
					.map(From::from);
				let thumbnail = SongFile::generate_thumbnail(&song, iter_pages());
				SongFile::save(
					file.get_path().unwrap(),
					song,
					iter_pages(),
					thumbnail,
					false,// TODO overwrite?!
				);
			}
		}
	}
}

#[derive(actix::Message)]
#[rtype(result = "()")]
struct StaffSelected(Option<usize>);

impl actix::Handler<StaffSelected> for AppActor {
	type Result = ();

	fn handle(&mut self, selected_staff: StaffSelected, ctx: &mut Self::Context) {
		let selected_staff = selected_staff.0;
		self.update_selection(selected_staff);
	}
}

#[derive(woab::BuilderSignal, Debug)]
enum AppSignal {
	/* Menu */
	NewDocument,
	OpenDocument,
	SaveDocument,
	/* Tool bar */
	AddPages,
	AddPages2,
	SelectPage,
	Autodetect,
	/* Editor */
	PieceStartUpdate(gtk::CheckButton),
	PieceNameUpdate(gtk::Entry),
	SectionStartUpdate(gtk::CheckButton),
	SectionEndUpdate(gtk::CheckButton),
	SectionRepetitionUpdate(gtk::CheckButton),
}

impl actix::StreamHandler<AppSignal> for AppActor {
	fn handle(&mut self, signal: AppSignal, ctx: &mut Self::Context) {
		println!("Signal: {:?}", signal);
		match signal {
			AppSignal::AddPages => self.add_pages(),
			AppSignal::SelectPage => {
				let selected_items = self.widgets.pages_preview.get_selected_items();
				self.select_page(match selected_items.len() {
					0 => None,
					1 => Some(PageIndex(selected_items[0].get_indices()[0] as usize)),
					_ => None,
				});
			},
			AppSignal::Autodetect => self.autodetect(ctx),
			AppSignal::PieceStartUpdate(piece_start) => self.update_part_start(piece_start.get_active()),
			AppSignal::PieceNameUpdate(piece_name) => self.update_part_name(&piece_name.get_text()),
			AppSignal::SectionStartUpdate(section_start) => self.update_section_start(section_start.get_active()),
			AppSignal::SectionEndUpdate(section_end) => self.update_section_end(section_end.get_active()),
			AppSignal::SectionRepetitionUpdate(section_repetition) => self.update_section_repetition(section_repetition.get_active()),
			AppSignal::OpenDocument => self.load_with_dialog(),
			AppSignal::NewDocument => self.unload_and_clear(),
			AppSignal::SaveDocument => self.save_with_ui(),
			_ => (),
		}
	}
}




fn main() -> Result<(), Box<dyn std::error::Error>> {
	let application = gtk::Application::new(
		Some("de.piegames.dinoscore.editor"),
		gio::ApplicationFlags::NON_UNIQUE,
	)
	.expect("Initialization failed...");

	application.connect_startup(|_application| {
		/* This is required so that builder can find this type. See gobject_sys::g_type_ensure */
		let _ = gio::ThemedIcon::static_type();
		libhandy::init();
		woab::run_actix_inside_gtk_event_loop("my-WoAB-app").unwrap(); // <===== IMPORTANT!!!
	});
	println!("D: {:?}", std::thread::current().id());

	application.connect_activate(move |application| {
		let builder = gtk::Builder::from_file("res/editor.glade");
		let builder = woab::BuilderConnector::from(builder);

		let app_builder = builder.actor::<AppActor>()
			.connect_signals(AppSignal::connector());

		let editor = builder.actor()
			.connect_signals(EditorSignal::connector())
			.start(EditorActor::new(todo!("app_builder.context().address()"), builder.widgets().unwrap()));

		app_builder.create(
			clone!(@weak application => @default-panic, move |ctx| {
				AppActor::new(ctx.widgets().unwrap(), application, editor)
			})
		);

		builder.finish();
	});

	application.run(&[]);
	Ok(())
}


/* #### Library foo #### */


/// Create a PDF Document with a single page that wraps a raster image
fn pixbuf_to_pdf(image: &gdk_pixbuf::Pixbuf) -> poppler::PopplerDocument {
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

	let pdf_binary = surface
		.finish_output_stream()
		.unwrap()
		.downcast::<Vec<u8>>()
		.unwrap();
	let pdf_binary: &mut [u8] = &mut *Box::leak(pdf_binary); // TODO: absolutely remove this

	poppler::PopplerDocument::new_from_data(pdf_binary, "").unwrap()
}

/// Render a PDF page to a preview image with fixed width
fn pdf_to_pixbuf(page: &poppler::PopplerPage, width: i32) -> gdk_pixbuf::Pixbuf {
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
