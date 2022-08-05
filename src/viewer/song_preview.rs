use dinoscore::{prelude::*, *};

glib::wrapper! {
	pub struct SongPreview(ObjectSubclass<imp::SongPreview>)
		@extends gtk::Box, gtk::Widget,
		@implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
					gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl SongPreview {
	pub fn init(
		&self,
		library: Rc<RefCell<library::Library>>,
		library_widget: crate::library_widget::LibraryWidget,
	) {
		self.imp().library.set(library).unwrap();
		self.imp().library_widget.set(library_widget).unwrap();
	}

	pub fn on_item_selected(&self, song: uuid::Uuid) {
		self.imp().on_item_selected(song);
	}
}

mod imp {
	use super::*;

	#[derive(CompositeTemplate, Default)]
	#[template(resource = "/de/piegames/dinoscore/viewer/song_preview.ui")]
	pub struct SongPreview {
		#[template_child]
		song_title: TemplateChild<gtk::Label>,
		#[template_child]
		song_composer: TemplateChild<gtk::Label>,
		#[template_child]
		part_preview: TemplateChild<adw::Carousel>,
		#[template_child]
		part_overlay: TemplateChild<gtk::Box>,
		#[template_child]
		part_name: TemplateChild<gtk::Label>,
		#[template_child]
		part_carousel_dots: TemplateChild<gtk::Widget>,
		#[template_child]
		stats_times_played: TemplateChild<gtk::Label>,
		#[template_child]
		stats_time_played: TemplateChild<gtk::Label>,
		#[template_child]
		stats_last_played: TemplateChild<gtk::Label>,

		pub library: OnceCell<Rc<RefCell<library::Library>>>,
		pub library_widget: OnceCell<crate::library_widget::LibraryWidget>,
		song_uuid: Cell<uuid::Uuid>,
		inhibit_autoscroll: Cell<bool>,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for SongPreview {
		const NAME: &'static str = "SongPreview";
		type Type = super::SongPreview;
		type ParentType = gtk::Box;

		fn class_init(klass: &mut Self::Class) {
			klass.bind_template();
			klass.bind_template_callbacks();
		}

		fn instance_init(obj: &InitializingObject<Self>) {
			obj.init_template();
		}
	}

	impl ObjectImpl for SongPreview {
		fn constructed(&self, obj: &Self::Type) {
			self.parent_constructed(obj);

			glib::source::timeout_add_seconds_local(
				10,
				clone!(@weak obj => @default-return glib::Continue(false), move || {
					obj.imp().on_timer();
					glib::Continue(true)
				}),
			);
		}
	}

	impl WidgetImpl for SongPreview {}

	impl BoxImpl for SongPreview {}

	#[gtk::template_callbacks]
	impl SongPreview {
		pub fn on_item_selected(&self, song: uuid::Uuid) {
			if song == self.song_uuid.get() {
				return;
			}
			self.song_uuid.set(song);

			let library = self.library.get().unwrap().borrow();
			let stats = library.stats.get(&song).unwrap();
			let song = library.songs.get(&song).unwrap();

			self.song_title
				.set_text(song.index.title.as_deref().unwrap_or("(no title)"));
			self.song_composer
				.set_text(song.index.composer.as_deref().unwrap_or("(no composer)"));

			/* Update preview carousel */
			let carousel = &self.part_preview.get();
			for page in (0..carousel.n_pages()).rev() {
				carousel.remove(&carousel.nth_page(page as u32));
			}

			for name in song.index.piece_starts.values() {
				let picture = gtk::Picture::builder()
					.paintable(&gdk::Paintable::new_empty(400, 100))
					.alternative_text(&name)
					.keep_aspect_ratio(true)
					.can_shrink(false)
					.build();
				carousel.append(&picture);
			}

			self.load_preview_background(song);

			/* Update stats */
			self.stats_times_played
				.set_label(&stats.times_played.to_string());
			self.stats_time_played
				.set_label(&format!("{:.1}", stats.seconds_played as f64 / 3600.0));
			self.stats_last_played.set_label(
				&stats
					.last_played
					.and_then(|last_played| {
						last_played
							.duration_since(std::time::SystemTime::UNIX_EPOCH)
							.ok()
					})
					.and_then(|last_played| {
						glib::DateTime::from_unix_local(last_played.as_secs() as i64).ok()
					})
					.and_then(|last_played| last_played.format("%_x").ok())
					.unwrap_or_else(|| "never".into()),
			);

			/* Reset the page and trigger an update */
			std::mem::drop(library);
			self.part_preview
				.scroll_to(&self.part_preview.nth_page(0), false);
		}

		/* The part_name label of the part_preview carousel is a floating overlay.
		 * Every time the page changes we need to update its text.
		 * We also update a few other related widgets here.
		 */
		#[template_callback]
		fn preview_page_changed(&self) {
			let library = self.library.get().unwrap().borrow();
			let song = library.songs.get(&self.song_uuid.get()).unwrap();

			let part_name = song
				.index
				.piece_starts
				.values()
				.nth(self.part_preview.position() as usize)
				.unwrap();
			self.part_name.set_label(part_name);
			self.part_overlay
				.set_visible(!part_name.is_empty() && self.part_preview.n_pages() > 1);
			/* We don't want a dozen dots when there are a lot of songs */
			self.part_carousel_dots
				.set_visible(self.part_preview.n_pages() < 6);
		}

		/* That's the big blue "play" button */
		#[template_callback]
		fn on_play_button_pressed(&self) {
			log::debug!("Activated (A)");
			self.library_widget
				.get()
				.unwrap()
				.load_song(self.song_uuid.get(), 0.into());
		}

		/* That's the small "▶" button next to the part_name */
		#[template_callback]
		fn on_quick_play_button_pressed(&self) {
			log::debug!("Activated (B)");
			let library = self.library.get().unwrap().borrow();
			let song = library.songs.get(&self.song_uuid.get()).unwrap();

			let start_at = *song
				.index
				.piece_starts
				.keys()
				.nth(self.part_preview.position() as usize)
				.unwrap();
			std::mem::drop(library);
			self.library_widget
				.get()
				.unwrap()
				.load_song(self.song_uuid.get(), start_at);
		}

		/** Called every 20 seconds
		 * Flip the page of the preview carousel, slide show style.
		 * Don't do that when the user has the mouse near it to not
		 * disrupt them.
		 */
		fn on_timer(&self) {
			let pages = self.part_preview.n_pages();
			if pages <= 1 || self.inhibit_autoscroll.get() {
				return;
			}
			let next_page = (self.part_preview.position() as u32 + 1) % pages;
			self.part_preview
				.scroll_to(&self.part_preview.nth_page(next_page), true);
		}

		#[template_callback]
		fn on_carousel_mouse_enter(&self) {
			self.inhibit_autoscroll.set(true);
		}

		#[template_callback]
		fn on_carousel_mouse_leave(&self) {
			self.inhibit_autoscroll.set(false);
		}

		/** Load the preview images of the parts on a background thread */
		fn load_preview_background(&self, song: &collection::SongFile) {
			let load_sheets = song.load_sheets();
			let meta = song.index.clone();
			let obj = Arc::new(fragile::Fragile::new(self.instance()));
			let uuid = self.song_uuid.get();

			std::thread::spawn(move || {
				let sheets = load_sheets().unwrap();

				for (index, &staff) in meta.piece_starts.keys().enumerate() {
					/* Render scaled preview images */
					let staff: &collection::Staff = &meta.staves[staff];
					let page: &PageImage = &sheets[staff.page];

					/* Prepare surface and fill background */
					let surface = cairo::ImageSurface::create(
						cairo::Format::Rgb24,
						400,
						(400.0 * staff.aspect_ratio()) as i32,
					)
					.unwrap();
					let context = cairo::Context::new(&surface).unwrap();
					context.set_antialias(cairo::Antialias::Best);
					context.set_source_rgb(1.0, 1.0, 1.0);
					context.paint().unwrap();

					let scale = 400.0 / staff.width();
					context.scale(scale, scale);
					context.translate(-staff.left(), -staff.top());
					context.scale(1.0 / page.reference_width(), 1.0 / page.reference_width());
					page.render_cairo(&context).unwrap();
					surface.flush();

					let pixbuf = gdk::pixbuf_get_from_surface(
						&surface,
						0,
						0,
						surface.width(),
						surface.height(),
					)
					.unwrap();
					let pixbuf = gdk::Texture::for_pixbuf(&pixbuf);

					/* Put them back into the carousel */
					let obj = obj.clone();
					glib::MainContext::default().spawn(async move {
						obj.get()
							.imp()
							.update_preview_image(uuid, index as u32, pixbuf);
					});
				}

				/* Make sure our fragile object gets dropped on the main thread */
				glib::MainContext::default().spawn(async move {
					std::mem::drop(obj);
				});
			});
		}

		fn update_preview_image(&self, song: uuid::Uuid, index: u32, image: gdk::Texture) {
			if song == self.song_uuid.get() {
				let picture = self
					.part_preview
					.nth_page(index)
					.downcast::<gtk::Picture>()
					.unwrap();
				picture.set_paintable(Some(&image));
			}
		}
	}
}
