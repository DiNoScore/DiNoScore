use dinoscore::{prelude::*, *};
use std::sync::mpsc::*;

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
		library_widget: crate::library_pane::LibraryPane,
	) {
		self.imp().library.set(library).unwrap();
		self.imp().library_widget.set(library_widget).unwrap();
		self.imp()
			.background_renderer
			.set(self.imp().spawn_background_renderer())
			.unwrap();
	}

	pub fn on_item_selected(&self, song: &crate::library_item::LibraryItem) {
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
		favorite: TemplateChild<gtk::ToggleButton>,
		favorite_binding: RefCell<Option<glib::Binding>>,
		#[template_child]
		tags: TemplateChild<adw::WrapBox>,

		#[template_child]
		stats_times_played: TemplateChild<adw::ActionRow>,
		#[template_child]
		stats_time_played: TemplateChild<adw::ActionRow>,
		#[template_child]
		stats_last_played: TemplateChild<adw::ActionRow>,

		pub library: OnceCell<Rc<RefCell<library::Library>>>,
		pub library_widget: OnceCell<crate::library_pane::LibraryPane>,
		song_uuid: Cell<uuid::Uuid>,
		inhibit_autoscroll: Cell<bool>,
		pub background_renderer: OnceCell<
			Sender<(
				uuid::Uuid,
				collection::SongMeta,
				Box<dyn FnOnce() -> anyhow::Result<TiVec<collection::PageIndex, PageImage>> + Send>,
			)>,
		>,
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
		fn constructed(&self) {
			self.parent_constructed();
			let obj = self.obj();

			glib::source::timeout_add_seconds_local(
				10,
				clone!(
					#[weak]
					obj,
					#[upgrade_or]
					glib::ControlFlow::Break,
					move || {
						obj.imp().on_timer();
						glib::ControlFlow::Continue
					}
				),
			);
		}
	}

	impl WidgetImpl for SongPreview {}

	impl BoxImpl for SongPreview {}

	#[gtk::template_callbacks]
	impl SongPreview {
		pub fn on_item_selected(&self, item: &crate::library_item::LibraryItem) {
			let song_uuid = item.uuid();
			let library = self.library.get().unwrap().borrow();
			let stats = library.stats.get(&song_uuid).unwrap();
			let song = library.songs.get(&song_uuid).unwrap();

			self.song_title
				.set_text(song.index.title.as_deref().unwrap_or("(no title)"));
			self.song_composer
				.set_text(song.index.composer.as_deref().unwrap_or("(no composer)"));

			/* Don't reload that bit if the song is the same */
			if song_uuid != self.song_uuid.get() {
				self.song_uuid.set(song_uuid);

				/* Update preview carousel */
				let carousel = &self.part_preview.get();
				for page in (0..carousel.n_pages()).rev() {
					carousel.remove(&carousel.nth_page(page as u32));
				}

				for name in song.index.piece_starts.values() {
					let picture = gtk::Picture::builder()
						.paintable(&gdk::Paintable::new_empty(400, 100))
						.alternative_text(name)
						.content_fit(gtk::ContentFit::Contain)
						.can_shrink(false)
						.build();
					carousel.append(&picture);
				}

				self.load_preview_background(song);

				/* Favorite stuff */

				let mut favorite_binding = self.favorite_binding.borrow_mut();
				if let Some(favorite_binding) = favorite_binding.as_ref() {
					favorite_binding.unbind();
				}
				*favorite_binding = Some(
					self.favorite
						.bind_property("active", item, "favorite")
						.build(),
				);
				/* Don't sync_create, because we sync into the other direction */
				self.favorite.set_active(item.favorite());

				/* Tags */
				/* TODO use that function in Libadwaita 1.8 */
				// self.tags.remove_all();
				while let Some(child) = self.tags.first_child() {
					self.tags.remove(&child);
				}
				let library_widget = self.library_widget.get().unwrap();
				for (kind, value) in song.index.tags() {
					let tag = crate::library_tag::LibraryTag::new(kind.into(), value.to_string());
					/* Connect click handler to toggle the filter bar tag */
					let kind_owned = kind.to_string();
					let value_owned = value.to_string();
					tag.connect_toggled(clone!(
						#[weak]
						library_widget,
						move |tag| {
							let tag_active = tag.is_active();
							let filter_active = library_widget.is_tag_active(&kind_owned, &value_owned);
							if tag_active != filter_active {
								library_widget.toggle_tag(&kind_owned, &value_owned);
							}
						}
					));
					self.tags.append(&tag);
				}
			}

			self.sync_preview_tags();

			/* Update stats */
			self.stats_times_played
				.set_subtitle(&stats.times_played.to_string());
			self.stats_time_played
				.set_subtitle(&format!("{:.1}", stats.seconds_played as f64 / 3600.0));
			self.stats_last_played.set_subtitle(
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

		#[template_callback]
		fn on_favorite_toggled(&self) {
			/* Reentrancy hack, I'm sorry */
			if let Ok(mut library) = self.library.get().unwrap().try_borrow_mut() {
				library
					.stats
					.get_mut(&self.song_uuid.get())
					.unwrap()
					.favorite = self.favorite.get().is_active();
				library.save_in_background();
				std::mem::drop(library);
				self.library_widget.get().unwrap().reload_songs_filtered();
			}
		}

		/// Sync preview tag counts and active states with the filter bar
		fn sync_preview_tags(&self) {
			let library_widget = self.library_widget.get().unwrap();
			let tag_counts = library_widget.get_tag_counts();

			for child in self
				.tags
				.observe_children()
				.into_iter()
				.map(Result::unwrap)
				.map(|obj| obj.downcast::<crate::library_tag::LibraryTag>().unwrap())
			{
				if let (Some(kind), Some(value)) = (child.kind(), child.value()) {
					child.set_count(
						tag_counts
							.get(&(kind.clone(), value.clone()))
							.copied()
							.unwrap_or_default(),
					);
					child.set_active(library_widget.is_tag_active(&kind, &value));
				}
			}
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
				.load_song(self.song_uuid.get(), 0);
		}

		/* That's the small "▶" button next to the part_name */
		#[template_callback]
		fn on_quick_play_button_pressed(&self) {
			log::debug!("Activated (B)");
			self.library_widget
				.get()
				.unwrap()
				.load_song(self.song_uuid.get(), self.part_preview.position() as u32);
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
			let uuid = self.song_uuid.get();

			self.background_renderer
				.get()
				.unwrap()
				.send((uuid, meta, Box::new(load_sheets)))
				.unwrap();
		}

		pub fn spawn_background_renderer<
			T: FnOnce() -> anyhow::Result<TiVec<collection::PageIndex, PageImage>> + Send + 'static,
		>(
			&self,
		) -> Sender<(uuid::Uuid, collection::SongMeta, T)> {
			let (in_tx, in_rx) = channel();
			let obj = Arc::new(fragile::Fragile::new(self.obj().clone()));

			/* We always only want the latest value */
			type Update<T> = (uuid::Uuid, collection::SongMeta, T);
			fn fetch_latest<T>(rx: &Receiver<Update<T>>) -> Option<Update<T>> {
				let mut last = None::<Update<T>>;
				loop {
					match rx.try_recv() {
						Ok(val) => {
							last = Some(val);
						},
						Err(TryRecvError::Empty) => {
							if let Some(last) = last {
								return Some(last);
							} else {
								/* Don't return empty handed */
								return rx.recv().ok();
							}
						},
						Err(TryRecvError::Disconnected) => return None,
					}
				}
			}

			std::thread::spawn(move || {
				loop {
					let Some((uuid, meta, load_sheets)) = fetch_latest::<T>(&in_rx) else {
						break;
					};

					let sheets = load_sheets().unwrap();

					for (index, &staff) in meta.piece_starts.keys().enumerate() {
						/* Render scaled preview images */
						let staff: &collection::Staff = &meta.staves[staff];
						let page: &PageImage = &sheets[staff.page];

						/* Prepare surface and fill background */
						let mut surface = cairo::ImageSurface::create(
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
						std::mem::drop(context);

						let bytes = glib::Bytes::from(&*surface.data().unwrap());
						let pixbuf = gdk::MemoryTexture::new(
							surface.width(),
							surface.height(),
							if cfg!(target_endian = "big") {
								gdk::MemoryFormat::X8r8g8b8
							} else {
								gdk::MemoryFormat::B8g8r8x8
							},
							&bytes,
							surface.stride() as usize,
						);

						/* Put them back into the carousel */
						let obj = obj.clone();
						glib::MainContext::default().spawn(async move {
							obj.get().imp().update_preview_image(
								uuid,
								index as u32,
								pixbuf.upcast(),
							);
						});
					}
				}
				/* Make sure our fragile object gets dropped on the main thread */
				glib::MainContext::default().spawn(async move {
					std::mem::drop(obj);
				});
			});

			in_tx
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
