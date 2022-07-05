#![windows_subsystem = "windows"]

use anyhow::Context;
use dinoscore::{prelude::*, *};

use dinoscore::{collection::*, prelude::*, *};

pub(self) mod editor;
pub(self) mod page;
use editor::*;

async fn yield_now() {
	struct YieldNow(bool);

	impl futures::Future for YieldNow {
		type Output = ();

		// The futures executor is implemented as a FIFO queue, so all this future
		// does is re-schedule the future back to the end of the queue, giving room
		// for other futures to progress.
		fn poll(
			mut self: std::pin::Pin<&mut Self>,
			cx: &mut std::task::Context<'_>,
		) -> std::task::Poll<Self::Output> {
			if !self.0 {
				self.0 = true;
				cx.waker().wake_by_ref();
				std::task::Poll::Pending
			} else {
				std::task::Poll::Ready(())
			}
		}
	}

	YieldNow(false).await
}

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
		add_button: TemplateChild<gtk::MenuButton>,

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
			self.add_button.popdown();
			let obj = self.instance();
			let filter = gtk::FileFilter::new();
			filter.add_mime_type("application/pdf");
			let choose = gtk::FileChooserNative::builder()
				.title("Select PDFs to load")
				.action(gtk::FileChooserAction::Open)
				.transient_for(&obj)
				.select_multiple(true)
				.filter(&filter)
				.build();

			run_async(
				&choose,
				clone!(@weak obj => @default-panic, move |choose, response| {
					if response == gtk::ResponseType::Accept {
						glib::MainContext::default().spawn_local_with_priority(
							glib::source::PRIORITY_DEFAULT_IDLE,
							clone!(@strong obj, @strong choose => async move {
								obj.clone().imp().load_pages(obj, choose, false).await;
							}),
						);
					}
				}),
			);
		}

		/// Show a dialog to load some images, then load them
		#[template_callback]
		pub fn add_pages2(&self) {
			self.add_button.popdown();
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
						glib::MainContext::default().spawn_local_with_priority(
							glib::source::PRIORITY_DEFAULT_IDLE,
							clone!(@strong obj, @strong choose => async move {
								obj.clone().imp().load_pages(obj, choose, true).await;
							}),
						);
					}
				}),
			);
		}

		async fn load_pages(
			&self,
			obj: <Self as ObjectSubclass>::Type,
			choose: gtk::FileChooserNative,
			/* Whether to extract all images from the PDFs because they are scans anyways */
			extract: bool,
		) {
			let (progress_dialog, progress) =
				dinoscore::create_progress_bar_dialog("Loading pages …", &obj);

			let total_work = choose.files().n_items() as f64;

			let mut pages = Vec::new();

			for (i, file) in choose
				.files()
				.snapshot()
				.iter()
				.map(|file| file.clone().downcast::<gio::File>().unwrap())
				.enumerate()
			{
				let path = file.path().unwrap();

				let (raw, path) = blocking::unblock(move || {
					let raw = std::fs::read(path.as_path()).unwrap();
					(raw, path)
				})
				.await;
				let extension = path.as_path().extension().and_then(std::ffi::OsStr::to_str);

				pages.extend(if let Some("pdf") = extension {
					if extract {
						let raw = image_util::extract_pdf_images_raw(&raw).unwrap();
						let total_pages = raw.len() as f64;
						let mut processed = Vec::with_capacity(raw.len());
						for (i2, (extension, raw)) in raw.into_iter().enumerate() {
							let image =
								gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(raw.clone()))
									.unwrap();
							processed.push(RawPageImage::Raster {
								image: image.render_to_thumbnail(1000).unwrap(),
								raw,
								extension,
							});

							progress.set_fraction(
								(i as f64 + ((i2 + 1) as f64) / total_pages) as f64 / total_work,
							);
							yield_now().await;
						}
						processed
					} else {
						image_util::explode_pdf(&raw, |raw, page| RawPageImage::Vector {
							page,
							raw,
						})
						.unwrap()
					}
				} else {
					let image = gdk_pixbuf::Pixbuf::from_file(&path).unwrap();
					vec![RawPageImage::Raster {
						image: image.render_to_thumbnail(1000).unwrap(),
						raw,
						extension: extension
							.expect("Image files must have an extension")
							.to_string(),
					}]
				});

				progress.set_fraction((i + 1) as f64 / total_work);
				yield_now().await;
			}

			progress.set_text(Some("Generating thumbnails…"));
			progress.set_fraction(0.0);
			progress.pulse();
			yield_now().await;

			// TODO clean up this mess
			let total_work = pages.len();

			for (i, page) in pages.into_iter().enumerate() {
				let thumbnail = page.render_to_thumbnail(400).unwrap();
				obj.imp().add_page_manual(page, thumbnail);
				progress.set_fraction((i + 1) as f64 / total_work as f64);
				yield_now().await;
			}
			yield_now().await;
			progress_dialog.emit_close();
		}

		/// Append a single loaded image to the end
		fn add_page(&self, page: RawPageImage) {
			let pixbuf = page.render_to_thumbnail(400).unwrap();
			self.add_page_manual(page, pixbuf);
		}

		/// Append a single loaded image to the end
		fn add_page_manual(&self, page: RawPageImage, thumbnail: gdk_pixbuf::Pixbuf) {
			self.file.borrow_mut().add_page(page);

			self.pages_preview_data
				.set(&self.pages_preview_data.append(), &[(0, &thumbnail)]);
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
			let obj = self.instance();

			let (progress_dialog, progress) =
				dinoscore::create_progress_bar_dialog("Detecting staves …", &obj);

			glib::MainContext::default().spawn_local_with_priority(
				glib::source::PRIORITY_DEFAULT_IDLE,
				async move {
					let total_work = selected_items.len();
					yield_now().await;

					for (i, page) in selected_items
						.into_iter()
						.map(|selected| selected.indices()[0] as usize)
						.enumerate()
					{
						let data: gdk_pixbuf::Pixbuf = obj.imp().pages_preview_data.get().get(
							&obj.imp()
								.pages_preview_data
								.iter_nth_child(None, page as i32)
								.unwrap(),
							0,
						);
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

						obj.imp().add_staves(page, bars_inner);
					}

					// tokio::time::sleep(std::time::Duration::from_millis(350)).await;
					progress_dialog.emit_close();
					yield_now().await;
					log::info!("Autodetected");
				},
			);
		}
	}
}

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

	#[cfg(debug_assertions)]
	{
		pipeline::pipe! {
			gvdb::gresource::GResourceXMLDocument::from_file("res/editor/resources.gresource.xml".as_ref()).unwrap()
			=> gvdb::gresource::GResourceBuilder::from_xml(_).unwrap()
			=> _.build().unwrap()
			=> glib::Bytes::from_owned
			=> &gio::Resource::from_data(&_)?
			=> gio::resources_register
		};
	}
	#[cfg(not(debug_assertions))]
	{
		pipeline::pipe! {
			gvdb_macros::include_gresource_from_xml!("res/editor/resources.gresource.xml")
			=> glib::Bytes::from_static
			=> &gio::Resource::from_data(&_)?
			=> gio::resources_register
		};
	}
	/* Vendor icons */
	gio::resources_register_include!("icons.gresource").context("Failed to register resources.")?;

	let application = gtk::Application::builder()
		.application_id("de.piegames.dinoscore.editor")
		.flags(gio::ApplicationFlags::NON_UNIQUE)
		.resource_base_path("/de/piegames/dinoscore")
		.build();

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
