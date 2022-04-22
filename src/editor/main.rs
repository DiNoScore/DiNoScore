use anyhow::Context;
use dinoscore::{prelude::*, *};

use dinoscore::{collection::*, prelude::*, *};

pub(self) mod editor;
pub(self) mod page;
use editor::*;

// TODO https://github.com/gtk-rs/gtk4-rs/issues/993
fn run_async<D: IsA<gtk::NativeDialog>, F: FnOnce(&D, gtk::ResponseType) + 'static>(
	this: &D,
	f: F,
) {
	let response_handler = Rc::new(RefCell::new(None));
	let response_handler_clone = response_handler.clone();
	let f = RefCell::new(Some(f));
	let this_clone = this.clone();
	*response_handler.borrow_mut() = Some(this.connect_response(move |s, response_type| {
		let _ = &this_clone;
		if let Some(handler) = response_handler_clone.borrow_mut().take() {
			s.disconnect(handler);
		}
		(*f.borrow_mut()).take().expect("cannot get callback")(s, response_type);
	}));
	this.show();
}

glib::wrapper! {
	pub struct EditorWindow(ObjectSubclass<imp::EditorWindow>)
		@extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
		@implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
					gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl EditorWindow {
	pub fn new(app: &Application) -> Self {
		let obj: Self = Object::new(&[("application", app)]).expect("Failed to create Window");
		obj.imp().init(&obj);
		obj
	}
}

mod imp {
	use super::*;

	#[derive(CompositeTemplate, Default)]
	#[template(resource = "/de/piegames/dinoscore/editor/window.ui")]
	pub struct EditorWindow {
		#[template_child]
		menubar: TemplateChild<gio::MenuModel>,

		#[template_child]
		pages_preview: TemplateChild<gtk::IconView>,
		/* Pixbufs preview cache */
		#[template_child(id = "store_pages")]
		pages_preview_data: TemplateChild<gtk::ListStore>,
		#[template_child]
		editor: TemplateChild<page::EditorPage>,

		file: Rc<RefCell<EditorSongFile>>,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for EditorWindow {
		const NAME: &'static str = "EditorWindow";
		type Type = super::EditorWindow;
		type ParentType = adw::ApplicationWindow;

		fn class_init(klass: &mut Self::Class) {
			klass.bind_template();
			klass.bind_template_callbacks();
		}

		fn instance_init(obj: &InitializingObject<Self>) {
			obj.init_template();
		}
	}

	impl ObjectImpl for EditorWindow {
		fn constructed(&self, obj: &Self::Type) {
			self.parent_constructed(obj);
			self.editor.init(self.file.clone());
		}
	}

	impl WidgetImpl for EditorWindow {}

	impl WindowImpl for EditorWindow {}

	impl ApplicationWindowImpl for EditorWindow {}

	impl AdwApplicationWindowImpl for EditorWindow {}

	#[gtk::template_callbacks]
	impl EditorWindow {
		pub fn init(&self, obj: &<Self as ObjectSubclass>::Type) {
			let application = obj.application().unwrap();

			let new = gio::SimpleAction::new("new", None);
			new.connect_activate(clone!(@weak obj => @default-panic, move |_, _| {
				obj.imp().unload_and_clear();
			}));
			application.add_action(&new);
			application.set_accels_for_action("app.new", &["<Primary>N"]);

			let open = gio::SimpleAction::new("open", None);
			open.connect_activate(clone!(@weak obj => @default-panic, move |_, _| {
				obj.imp().load_with_dialog();
			}));
			application.add_action(&open);
			application.set_accels_for_action("app.open", &["<Primary>O"]);

			let save = gio::SimpleAction::new("save", None);
			save.connect_activate(clone!(@weak obj => @default-panic, move |_, _| {
				obj.imp().save_with_ui();
			}));
			application.add_action(&save);
			application.set_accels_for_action("app.save", &["<Primary>S"]);

			application.set_accels_for_action("window.close", &["<Primary>Q"]);

			/* Enforce some invariants */
			self.unload_and_clear();
		}

		fn unload_and_clear(&self) {
			self.pages_preview_data.clear();
			*self.file.borrow_mut() = EditorSongFile::new();
		}

		fn load_with_dialog(&self) {
			let obj = self.instance();
			let filter = gtk::FileFilter::new();
			filter.add_mime_type("application/zip");
			let choose = gtk::FileChooserNative::builder()
				.title("File to load")
				.action(gtk::FileChooserAction::Open)
				.modal(true)
				.select_multiple(false)
				.transient_for(&obj)
				.filter(&filter)
				.build();
			choose.show();

			choose.connect_response(
				clone!(@weak obj, @strong choose => @default-panic, move |_, response| {
					if response == gtk::ResponseType::Accept {
						if let Some(file) = choose.file() {
							let path = file.path().unwrap();
							let mut song = SongFile::new(path).unwrap();
							obj.imp().load(song.load_sheets_raw().unwrap(), song.index);
						}
					}
				}),
			);
		}

		fn load(&self, pages: TiVec<PageIndex, RawPageImage>, song: SongMeta) {
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
			self.file.borrow_mut().piece_starts = song.piece_starts;
			self.file.borrow_mut().section_starts = song.section_starts;
		}

		fn save_with_ui(&self) {
			log::info!("Saving staves");

			let obj = self.instance();
			let filter = gtk::FileFilter::new();
			filter.add_mime_type("application/zip");
			let choose = gtk::FileChooserNative::builder()
				.title("Save song")
				.action(gtk::FileChooserAction::Save)
				.transient_for(&obj)
				.select_multiple(false)
				.filter(&filter)
				.build();

			run_async(
				&choose,
				clone!(@weak obj => @default-panic, move |choose, response| {
					if response == gtk::ResponseType::Accept {
						if let Some(file) = choose.file() {
							obj.imp().file.borrow().save(file.path().unwrap()).unwrap();
						}
					}
				}),
			);
		}

		#[template_callback]
		fn on_key(&self, keyval: gdk::Key) {
			if keyval == gdk::Key::Delete || keyval == gdk::Key::KP_Delete {
				let selected_items = self.pages_preview.selected_items();
				selected_items
					.iter()
					.map(|selected| selected.indices()[0] as usize)
					.for_each(|i| {
						self.remove_page(PageIndex(i));
					});
			}
		}

		fn remove_page(&self, page: PageIndex) {
			self.file.borrow_mut().remove_page(page);

			self.pages_preview_data.remove(
				&self
					.pages_preview_data
					.iter(&gtk::TreePath::from_indices(&[*page as i32]))
					.unwrap(),
			);
		}

		/// Show a dialog to load some images, then load them
		#[template_callback]
		pub fn add_pages(&self) {
			let obj = self.instance();
			let filter = gtk::FileFilter::new();
			filter.add_pixbuf_formats();
			filter.add_mime_type("application/pdf");
			let choose = gtk::FileChooserNative::builder()
				.title("Select images or PDFs to load")
				.action(gtk::FileChooserAction::Open)
				.transient_for(&obj)
				.select_multiple(true)
				.filter(&filter)
				.build();

			run_async(
				&choose,
				clone!(@weak obj => @default-panic, move |choose, response| {
					if response == gtk::ResponseType::Accept {
						// TODO clean up a bit
						glib::MainContext::default().spawn_local_with_priority(
							glib::source::PRIORITY_DEFAULT_IDLE,
							clone!(@strong obj, @strong choose => async move {
								let (progress_dialog, progress) =
									dinoscore::create_progress_bar_dialog("Loading pages …");
								let total_work = choose.files().n_items();

								for (i, file) in choose.files()
										.snapshot()
										.iter()
										.map(|file| file.clone().downcast::<gio::File>().unwrap())
										.enumerate() {
									let path = file.path().unwrap();

									let pages = blocking::unblock(move || {
										let raw = std::fs::read(path.as_path()).unwrap();
										let extension =
											path.as_path().extension().and_then(std::ffi::OsStr::to_str);
										let pages = if let Some("pdf") = extension {
											image_util::explode_pdf_full(&raw, |raw, page| {
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

									for page in unsafe { pages.unwrap() } {
										obj.imp().add_page(page);
									}
								}
								// tokio::time::sleep(std::time::Duration::from_millis(350)).await;
								progress_dialog.emit_close();
							}),
						);
					}
				}),
			);
		}

		/// Append a single loaded image to the end
		fn add_page(&self, page: RawPageImage) {
			let pixbuf = page.render_to_thumbnail(400).unwrap();
			self.file.borrow_mut().add_page(page);

			self.pages_preview_data
				.set(&self.pages_preview_data.append(), &[(0, &pixbuf)]);
		}

		/// Callback from the icon view
		#[template_callback]
		fn page_changed(&self) {
			let selected_items = self.pages_preview.selected_items();
			log::debug!("Selection changed: {} items", selected_items.len());
			let selected_page = match selected_items.len() {
				0 => None,
				1 => Some(PageIndex(selected_items[0].indices()[0] as usize)),
				_ => None,
			};
			self.editor.load_page(selected_page);
		}

		fn add_staves(&self, page_index: PageIndex, staves: Vec<Staff>) {
			self.file.borrow_mut().add_staves(page_index, staves);
			self.editor.update_page();
		}

		#[template_callback]
		fn autodetect(&self) {
			let selected_items = self.pages_preview.selected_items();

			let pdf_pages = self.file.borrow().get_pages();

			// ctx.spawn(actix::fut::wrap_future(async move {
			// let (progress_dialog, progress) =
			// 	dinoscore::create_progress_bar_dialog("Detecting staves …");
			let total_work = selected_items.len();
			// tokio::task::yield_now().await;

			for (i, page) in selected_items
				.into_iter()
				.map(|selected| selected.indices()[0] as usize)
				.enumerate()
			{
				let data: gdk_pixbuf::Pixbuf = self.pages_preview_data.get().get(
					&self
						.pages_preview_data
						.iter_nth_child(None, page as i32)
						.unwrap(),
					0,
				);
				let pdf_page = &pdf_pages[page];
				let width = pdf_page.get_width() as f64;
				let height = pdf_page.get_height() as f64;

				let data = unsafe { unsafe_force::Send::new(data) };
				let (page, bars_inner) = /*blocking::unblock(move ||*/ {
					log::info!("Autodetecting {} ({}/{})", page, i, total_work);
					let page = PageIndex(page);
					let bars_inner: Vec<Staff> = tokio::runtime::Builder::new_current_thread().enable_time().enable_io().build().unwrap().block_on(async {
						recognition::recognize_staves(&unsafe { data.unwrap() })
						.await
					})
						.iter()
						.cloned()
						.map(|staff| staff.into_staff(page, width, height))
						.collect();
					(page, bars_inner)
				}/*)
				.await*/;
				// progress.set_fraction((i + 1) as f64 / total_work as f64);

				self.add_staves(page, bars_inner);
			}

			// tokio::time::sleep(std::time::Duration::from_millis(350)).await;
			// progress_dialog.emit_close();
			// tokio::task::yield_now().await;
			log::info!("Autodetected");
			// }));
		}
	}
}

// 	fn add_pages2(&mut self) {
// 		// 				let selected_items = pages_preview.get_selected_items();
// 		// 				selected_items.iter()
// 		// 					.map(|selected| selected.get_indices()[0] as usize)
// 		// 					.for_each(|i| {
// 		// 						let bars_inner = vec![Staff {
// 		// 							left: 0.0, right: 1.0, top: 0.0, bottom: 1.0,
// 		// 						}];
// 		// 						state.borrow_mut().add_staves(i.into(), bars_inner);
// 		// 					});

// 		// let filter = gtk::FileFilter::new();
// 		// filter.add_pixbuf_formats();
// 		// let choose = gtk::FileChooserNativeBuilder::new()
// 		// 	.title("Select images to load")
// 		// 	.action(gtk::FileChooserAction::Open)
// 		// 	.select_multiple(true)
// 		// 	.filter(&filter)
// 		// 	.build();
// 		// if choose.run() == gtk::ResponseType::Accept {
// 		// 	for file in choose.get_files() {
// 		// 		let path = file.get_path().unwrap();
// 		// 		let image = opencv::imgcodecs::imread(&path.to_str().unwrap(), 0).unwrap();

// 		// 		let mut image_binarized = opencv::core::Mat::default().unwrap();
// 		// 		opencv::imgproc::adaptive_threshold(&image, &mut image_binarized, 255.0,
// 		// 			opencv::imgproc::AdaptiveThresholdTypes::ADAPTIVE_THRESH_MEAN_C as i32,
// 		// 			opencv::imgproc::ThresholdTypes::THRESH_BINARY as i32,
// 		// 			101, 30.0
// 		// 		).unwrap();

// 		// 		let mut image_binarized_median = opencv::core::Mat::default().unwrap();
// 		// 		opencv::imgproc::median_blur(&image_binarized, &mut image_binarized_median, 3).unwrap();

// 		// 		dbg!(opencv::imgcodecs::imwrite("./tmp.png", &image_binarized_median, &opencv::core::Vector::new()).unwrap());
// 		// 		/* The easiest way to convert Mat to Pixbuf is to write it to a PNG buffer */
// 		// 		let mut png = opencv::core::Vector::new();
// 		// 		dbg!(opencv::imgcodecs::imencode(
// 		// 			".png",
// 		// 			&image_binarized_median,
// 		// 			&mut png,
// 		// 			&opencv::core::Vector::new(),
// 		// 		).unwrap());
// 		// 		let pixbuf = gdk_pixbuf::Pixbuf::from_stream(
// 		// 			/* How many type conversion layers will we pile today? */
// 		// 			&gio::MemoryInputStream::from_bytes(&glib::Bytes::from(&png.to_vec())),
// 		// 			Option::<&gio::Cancellable>::None,
// 		// 		).unwrap();
// 		// 		let pdf = pixbuf_to_pdf(&pixbuf);
// 		// 		for page in 0..pdf.get_n_pages() {
// 		// 			let page = pdf.get_page(page).unwrap();
// 		// 			state.borrow_mut().add_page(page);
// 		// 		}
// 		// 	}
// 		// }
// 	}

#[allow(clippy::all)]
fn main() -> anyhow::Result<()> {
	fern::Dispatch::new()
		.format(
			fern::formatter::FormatterBuilder::default()
				.color_config(|config| {
					config
						.debug(fern::colors::Color::Magenta)
						.trace(fern::colors::Color::BrightMagenta)
				})
				.build(),
		)
		.level(log::LevelFilter::Trace)
		.chain(fern::logger::stdout())
		.apply()
		.context("Failed to initialize logger")?;

	glib::log_set_default_handler(glib::rust_log_handler);

	let orig_hook = std::panic::take_hook();
	std::panic::set_hook(Box::new(move |panic_info| {
		/* invoke the default handler and exit the process */
		orig_hook(panic_info);
		std::process::exit(1);
	}));

	gio::resources_register_include!("editor.gresource").expect("Failed to register resources.");

	let application = gtk::Application::new(
		Some("de.piegames.dinoscore.editor"),
		gio::ApplicationFlags::NON_UNIQUE,
	);

	application.connect_startup(|_application| {
		/* This is required so that builder can find this type. See gobject_sys::g_type_ensure */
		let _ = gio::ThemedIcon::static_type();
		let _ = page::EditorPage::static_type();
		adw::init();
	});

	application.connect_activate(move |application| {
		let window = EditorWindow::new(application);
		window.present();

		log::info!("Application started");
	});

	application.run_with_args(&[] as &[&str]);
	log::info!("Thanks for using DiNoScore.");
	Ok(())
}
