#![windows_subsystem = "windows"]

use anyhow::Context;
use dinoscore::{prelude::*, *};

use dinoscore::{collection::*, prelude::*, *};

pub(self) mod editor;
pub(self) mod page;
#[cfg(test)]
mod screenshots;
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
		add_button: TemplateChild<adw::SplitButton>,

		#[template_child]
		pub pages_preview: TemplateChild<gtk::IconView>,
		/* Pixbufs preview cache */
		#[template_child(id = "store_pages")]
		pages_preview_data: TemplateChild<gtk::ListStore>,
		#[template_child]
		autodetect: TemplateChild<gtk::Button>,
		#[template_child]
		pub editor: TemplateChild<page::EditorPage>,
		#[template_child]
		song_name: TemplateChild<gtk::Entry>,
		#[template_child]
		song_composer: TemplateChild<gtk::Entry>,

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
			self.song_name.set_text("");
			self.song_composer.set_text("");
			self.add_button
				.style_context()
				.add_class("suggested-action");
			self.autodetect
				.style_context()
				.remove_class("suggested-action");
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

			run_async(
				&choose,
				clone!(@weak obj => @default-panic, move |choose, response| {
					if response == gtk::ResponseType::Accept {
						if let Some(file) = choose.file() {
							let path = file.path().unwrap();
							let progress_dialog = dinoscore::create_progress_spinner_dialog("Loading pages …", &obj);
							glib::MainContext::default().spawn_local_with_priority(
								glib::source::PRIORITY_DEFAULT_IDLE,
								clone!(@strong obj, @strong choose => async move {
									yield_now().await;

									let song = SongFile::new(path, &mut Default::default()).unwrap();
									let load_sheets = song.load_sheets();
									let sheets = blocking::unblock(move || load_sheets()).await.unwrap();
									obj.imp().load(sheets, song.index);

									yield_now().await;
									progress_dialog.emit_close();
								}),
							);
						}
					}
				}),
			);
		}

		pub fn load(&self, pages: TiVec<PageIndex, PageImage>, song: SongMeta) {
			self.unload_and_clear();
			for page in pages {
				self.add_page(page);
			}

			self.song_name.set_text(song.title.as_deref().unwrap_or(""));
			self.song_composer
				.set_text(song.composer.as_deref().unwrap_or(""));

			self.file.borrow_mut().load(song);

			self.editor.update_page();
		}

		fn save_with_ui(&self) {
			log::info!("Saving staves");

			let obj = self.instance();

			if self.file.borrow().get_staves().len() == 0 {
				let dialog = gtk::MessageDialog::new(
					Some(&obj),
					gtk::DialogFlags::MODAL,
					gtk::MessageType::Error,
					gtk::ButtonsType::Ok,
					"You need to add least one staff annotation before saving",
				);
				dialog.set_default_response(gtk::ResponseType::Ok);
				dialog.connect_response(|dialog, _response| dialog.close());
				dialog.present();
				return;
			}

			let filter = gtk::FileFilter::new();
			filter.add_mime_type("application/zip");
			let choose = gtk::FileChooserNative::builder()
				.title("Save song")
				.action(gtk::FileChooserAction::Save)
				.transient_for(&obj)
				.select_multiple(false)
				.filter(&filter)
				.build();

			let title = &self.file.borrow().song_name;
			let composer = &self.file.borrow().song_composer;
			match (title.is_empty(), composer.is_empty()) {
				(false, false) => choose.set_current_name(&format!("{composer} – {title}.zip")),
				(false, true) => choose.set_current_name(&format!("{title}.zip")),
				_ => {},
			}

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
								obj.clone().imp().load_pages(&obj, choose
									.files()
									.snapshot()
									.iter()
									.map(|file| file.clone().downcast::<gio::File>().unwrap()), false).await;
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
								obj.clone().imp().load_pages(&obj, choose
									.files()
									.snapshot()
									.iter()
									.map(|file| file.clone().downcast::<gio::File>().unwrap()), true).await;
							}),
						);
					}
				}),
			);
		}

		pub async fn load_pages(
			&self,
			obj: &<Self as ObjectSubclass>::Type,
			files: impl ExactSizeIterator<Item = gio::File>,
			/* Whether to extract all images from the PDFs because they are scans anyways */
			extract: bool,
		) {
			let (progress_dialog, progress) =
				dinoscore::create_progress_bar_dialog("Loading pages …", obj);
			yield_now().await;

			let total_work = files.len() as f64;

			let mut pages = Vec::new();

			/* Warn the user if the import did not yield the expected amount of pages */
			let mut warn_pages = false;

			for (i, file) in files.enumerate() {
				let path = file.path().unwrap();

				let (raw, path) = blocking::unblock(move || {
					let raw = std::fs::read(path.as_path()).unwrap();
					(raw, path)
				})
				.await;
				let extension = path.as_path().extension().and_then(std::ffi::OsStr::to_str);

				pages.extend(if let Some("pdf") = extension {
					if extract {
						let (raw, pdf_pages) = image_util::extract_pdf_images_raw(&raw).unwrap();
						let total_pages = raw.len() as f64;
						warn_pages |= pdf_pages != raw.len();
						let mut processed = Vec::with_capacity(raw.len());
						for (i2, (extension, raw)) in raw.into_iter().enumerate() {
							processed.push(PageImage::from_image(raw, extension).unwrap());

							progress.set_fraction(
								(i as f64 + ((i2 + 1) as f64) / total_pages) as f64 / total_work,
							);
							yield_now().await;
						}
						processed
					} else {
						image_util::explode_pdf(&raw)
							.unwrap()
							.map(|result| {
								let (raw, _) = result?;
								PageImage::from_pdf(raw)
							})
							.collect::<anyhow::Result<Vec<_>>>()
							.unwrap()
					}
				} else {
					vec![PageImage::from_image(
						raw,
						extension
							.expect("Image files must have an extension")
							.to_string(),
					)
					.unwrap()]
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
				let thumbnail = page.render_scaled(400);
				obj.imp().add_page_manual(page, thumbnail);
				progress.set_fraction((i + 1) as f64 / total_work as f64);
				yield_now().await;
			}
			yield_now().await;
			progress_dialog.emit_close();

			if warn_pages {
				let dialog = gtk::MessageDialog::new(
					Some(obj),
					gtk::DialogFlags::MODAL,
					gtk::MessageType::Warning,
					gtk::ButtonsType::Ok,
					"Extracting PDF images did not yield exactly one image per page, so be prepared for weird results. If they are not satisfying, try importing the PDF as vector graphic, or extract the images with an external tool first.",
				);
				dialog.set_default_response(gtk::ResponseType::Ok);
				dialog.connect_response(|dialog, _response| dialog.close());
				dialog.present();
			}
		}

		/// Append a single loaded image to the end
		fn add_page(&self, page: PageImage) {
			let pixbuf = page.render_scaled(400);
			self.add_page_manual(page, pixbuf);
		}

		/// Append a single loaded image to the end
		fn add_page_manual(&self, page: PageImage, thumbnail: gdk_pixbuf::Pixbuf) {
			self.add_button
				.style_context()
				.remove_class("suggested-action");
			if self.file.borrow().get_pages().is_empty() {
				self.autodetect
					.style_context()
					.add_class("suggested-action");
			}

			self.file.borrow_mut().add_page(page);

			self.pages_preview_data
				.set(&self.pages_preview_data.append(), &[(0, &thumbnail)]);
		}

		/// Callback from the icon view
		#[template_callback]
		pub fn page_changed(&self) {
			let selected_items = self.pages_preview.selected_items();
			log::debug!("Selection changed: {} items", selected_items.len());
			let selected_page = match selected_items.len() {
				0 => None,
				1 => Some(PageIndex(selected_items[0].indices()[0] as usize)),
				_ => None,
			};
			self.autodetect.set_sensitive(!selected_items.is_empty());
			self.editor.load_page(selected_page);
		}

		fn add_staves(&self, page_index: PageIndex, staves: Vec<Staff>) {
			self.file.borrow_mut().add_staves(page_index, staves);
			self.editor.update_page();
		}

		#[template_callback]
		pub fn autodetect(&self) {
			self.autodetect
				.style_context()
				.remove_class("suggested-action");

			let selected_items = self
				.pages_preview
				.selected_items()
				.into_iter()
				.map(|selected| selected.indices()[0] as usize)
				.collect::<std::collections::BTreeSet<_>>();

			let obj = self.instance();

			let (progress_dialog, progress) =
				dinoscore::create_progress_bar_dialog("Detecting staves …", &obj);

			glib::MainContext::default().spawn_local_with_priority(
				glib::source::PRIORITY_DEFAULT_IDLE,
				async move {
					let total_work = selected_items.len();
					yield_now().await;

					for (i, page) in selected_items.into_iter().enumerate() {
						let data: gdk_pixbuf::Pixbuf = obj.imp().pages_preview_data.get().get(
							&obj.imp()
								.pages_preview_data
								.iter_nth_child(None, page as i32)
								.unwrap(),
							0,
						);

						// TODO already convert pixbuf to bytes here, then remove the unsafe
						let data = unsafe { unsafe_force::Send::new(data) };
						let (page, bars_inner) = blocking::unblock(move || {
							log::info!("Autodetecting {} ({}/{})", page, i, total_work);
							let page = PageIndex(page);
							let bars_inner: Vec<Staff> =
								recognition::recognize_staves(&unsafe { data.unwrap() }, page);
							log::debug!("Found {} staves", bars_inner.len());
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

		#[template_callback]
		fn update_song_name(&self) {
			self.file.borrow_mut().song_name = self.song_name.text().to_string();
		}

		#[template_callback]
		fn update_song_composer(&self) {
			self.file.borrow_mut().song_composer = self.song_composer.text().to_string();
		}
	}
}

fn gtk_init(_application: &gtk::Application) {
	/* This is required so that builder can find this type. See gobject_sys::g_type_ensure */
	let _ = gio::ThemedIcon::static_type();
	let _ = page::EditorPage::static_type();
	adw::init();
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
		.level_for("multipart", log::LevelFilter::Info)
		.level_for("serde_xml_rs", log::LevelFilter::Info)
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

	application.connect_startup(gtk_init);

	application.connect_activate(move |application| {
		let window = EditorWindow::new(application);
		window.present();

		log::info!("Application started");

		/* Load some test data for debugging (enable by hard-coding) */
		if cfg!(any()) {
			glib::MainContext::default().spawn_local_with_priority(
				glib::source::PRIORITY_DEFAULT_IDLE,
				async move {
					/* Load pages */
					window.clone().imp().load_pages(&window, [
						gio::File::for_path("test/recognition/Beethoven, Ludwig van – Piano Sonata No.2, Op.2 No.2.pdf"),
						gio::File::for_path("test/recognition/Saint-Saëns, Camille – Danse macabre, Op.40.pdf"),
					].into_iter(), false).await;

					/* Auto-auto-detect */
					let imp = window.imp();
					imp.pages_preview.select_all();
					imp.autodetect();
				},
			);
		}
	});

	application.run_with_args(&[] as &[&str]);
	log::info!("Thanks for using DiNoScore.");
	Ok(())
}
