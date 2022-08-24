use dinoscore::{library::ScaleMode, prelude::*, *};

use std::sync::mpsc::*;

glib::wrapper! {
	pub struct SongWidget(ObjectSubclass<imp::SongWidget>)
		@extends gtk::Box, gtk::Widget,
		@implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
					gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl SongWidget {
	pub fn init(&self, library: Rc<RefCell<library::Library>>) {
		self.imp().library.set(library).unwrap();
	}

	pub fn load_song(
		&self,
		song: collection::SongMeta,
		pages: TiVec<collection::PageIndex, PageImage>,
		scale_mode: ScaleMode,
		start_at: collection::StaffIndex,
	) {
		self.imp()
			.load_song(song, Arc::new(pages), scale_mode, start_at);
	}

	#[cfg(test)]
	pub fn part_selection(&self) -> gtk::ComboBoxText {
		self.imp().part_selection.get()
	}

	#[cfg(test)]
	pub fn zoom_button(&self) -> gtk::MenuButton {
		self.imp().zoom_button.get()
	}

	#[cfg(test)]
	pub fn set_zoom_mode(&self, mode: &str) {
		self.imp().scale_mode_changed(&mode.to_variant());
	}
}

mod imp {
	use super::*;

	#[derive(CompositeTemplate)]
	#[template(resource = "/de/piegames/dinoscore/viewer/song.ui")]
	pub struct SongWidget {
		#[template_child]
		header: TemplateChild<adw::HeaderBar>,
		#[template_child]
		carousel: TemplateChild<adw::Carousel>,
		/* Hack to get resize notifications for the carousel (it is transparent and overlaid) */
		#[template_child]
		size_catcher: TemplateChild<gtk::DrawingArea>,
		#[template_child]
		song_progress: TemplateChild<gtk::ProgressBar>,
		#[template_child]
		pub part_selection: TemplateChild<gtk::ComboBoxText>,
		/* Needed to inhibit that signal sometimes. */
		part_selection_changed_signal: OnceCell<glib::SignalHandlerId>,
		#[template_child]
		pub zoom_button: TemplateChild<gtk::MenuButton>,

		pub library: OnceCell<Rc<RefCell<library::Library>>>,
		song: RefCell<Option<SongState>>,

		actions: gio::SimpleActionGroup,
		/* Navigation */
		next: gio::SimpleAction,
		previous: gio::SimpleAction,
		next_piece: gio::SimpleAction,
		previous_piece: gio::SimpleAction,
		/* Zoom */
		#[template_child]
		zoom_gesture: TemplateChild<gtk::GestureZoom>,
		#[template_child]
		scroll_gesture: TemplateChild<gtk::EventControllerScroll>,
		sizing_mode_action: gio::SimpleAction,

		last_interaction: Cell<std::time::Instant>,
		/// Some when loading a song. After 90 seconds, we increment the load count and set to None
		song_load_time: Cell<Option<std::time::Instant>>,

		hide_cursor: RefCell<Option<glib::source::SourceId>>,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for SongWidget {
		const NAME: &'static str = "ViewerSong";
		type Type = super::SongWidget;
		type ParentType = gtk::Box;

		fn new() -> Self {
			let actions = gio::SimpleActionGroup::new();

			let next = gio::SimpleAction::new("next-page", None);
			let previous = gio::SimpleAction::new("previous-page", None);
			let next_piece = gio::SimpleAction::new("next-piece", None);
			let previous_piece = gio::SimpleAction::new("previous-piece", None);
			actions.add_action(&next);
			actions.add_action(&previous);
			actions.add_action(&previous_piece);
			actions.add_action(&next_piece);

			let sizing_mode_action = gio::SimpleAction::new_stateful(
				"sizing-mode",
				Some(&String::static_variant_type()),
				&"manual".to_variant(),
			);
			actions.add_action(&sizing_mode_action);

			SongWidget {
				header: Default::default(),
				carousel: Default::default(),
				size_catcher: Default::default(),
				song_progress: Default::default(),
				part_selection: Default::default(),
				part_selection_changed_signal: Default::default(),
				zoom_button: Default::default(),
				library: Default::default(),
				song: Default::default(),

				actions,
				next,
				previous,
				next_piece,
				previous_piece,
				zoom_gesture: Default::default(),
				scroll_gesture: Default::default(),

				sizing_mode_action,
				last_interaction: std::time::Instant::now().into(),
				song_load_time: Default::default(),

				hide_cursor: Default::default(),
			}
		}

		fn class_init(klass: &mut Self::Class) {
			klass.bind_template();
			klass.bind_template_callbacks();

			klass.install_action("song.zoom-in", None, move |obj, _, _| {
				obj.imp().zoom_in();
			});
			klass.install_action("song.zoom-out", None, move |obj, _, _| {
				obj.imp().zoom_out();
			});
			klass.install_action("song.zoom-original", None, move |obj, _, _| {
				obj.imp().zoom_reset();
			});
		}

		fn instance_init(obj: &InitializingObject<Self>) {
			obj.init_template();
		}
	}

	impl ObjectImpl for SongWidget {
		fn properties() -> &'static [glib::ParamSpec] {
			Box::leak(Box::new([
				glib::ParamSpecString::new(
					"song-name",                /* name */
					"song-name",                /* nickname */
					"name",                     /* "blurb" (?) */
					None,                       /* default */
					glib::ParamFlags::READABLE, /* read-only */
				),
				glib::ParamSpecString::new(
					"song-id",                  /* name */
					"song-id",                  /* nickname */
					"uuid",                     /* "blurb" (?) */
					None,                       /* default */
					glib::ParamFlags::READABLE, /* read-only */
				),
			]))
		}

		fn property(&self, _obj: &Self::Type, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
			match pspec.name() {
				"song-name" => self
					.song
					.borrow()
					.as_ref()
					.and_then(|song| song.song.title.as_ref())
					.to_value(),
				"song-id" => self
					.song
					.borrow()
					.as_ref()
					.map(|song| song.song.song_uuid.to_string())
					.to_value(),
				_ => unimplemented!(),
			}
		}

		fn constructed(&self, obj: &Self::Type) {
			self.parent_constructed(obj);

			obj.insert_action_group("song", Some(&self.actions));

			self.part_selection_changed_signal
				.set(
					self.part_selection
						.connect_changed(clone_!(self, move |obj, _| {
							obj.imp().select_part();
						})),
				)
				.unwrap();

			self.next
				.connect_activate(clone_!(self, move |obj, _a, _p| {
					obj.imp().next_page();
				}));
			self.previous
				.connect_activate(clone_!(self, move |obj, _a, _p| {
					obj.imp().previous_page();
				}));
			self.next_piece
				.connect_activate(clone_!(self, move |obj, _a, _p| {
					obj.imp().next_piece();
				}));
			self.previous_piece
				.connect_activate(clone_!(self, move |obj, _a, _p| {
					obj.imp().previous_piece();
				}));
			self.sizing_mode_action
				.connect_activate(clone_!(self, move |obj, _a, p| {
					obj.imp().scale_mode_changed(p.unwrap());
				}));

			let hide_mouse_controller = gtk4::EventControllerMotion::new();
			hide_mouse_controller.connect_enter(clone_!(self, move |obj, _, _x, _y| {
				obj.imp().restart_cursor_timer();
			}));
			hide_mouse_controller.connect_leave(clone_!(self, move |obj, _| {
				obj.imp().stop_cursor_timer();
			}));
			hide_mouse_controller.connect_motion(clone_!(self, move |obj, _, _x, _y| {
				obj.imp().restart_cursor_timer();
			}));
			self.carousel.add_controller(&hide_mouse_controller);

			/* MIDI handling */
			#[cfg(unix)]
			{
				let (midi_tx, midi_rx) = glib::MainContext::channel(glib::Priority::default());
				let handler = crate::pedal::run(midi_tx).unwrap();
				midi_rx.attach(
					None,
					clone!(@weak obj => @default-return Continue(false), move |event| {
						/* Reference the MIDI handler which holds the Sender so that it doesn't get dropped. */
						let _handler = &handler;
						match event {
							crate::pedal::PageEvent::Next => {
								obj.imp().next.activate(None);
							},
							crate::pedal::PageEvent::Previous => {
								obj.imp().previous.activate(None);
							},
						}
						Continue(true)
					}),
				);
			}
		}
	}

	impl WidgetImpl for SongWidget {}

	impl BoxImpl for SongWidget {}

	#[gtk::template_callbacks]
	impl SongWidget {
		pub fn load_song(
			&self,
			song: collection::SongMeta,
			pages: Arc<TiVec<collection::PageIndex, PageImage>>,
			scale_mode: ScaleMode,
			start_at: collection::StaffIndex,
		) {
			log::debug!("Loading song");
			let song = Arc::new(song);
			let (renderer, update_page) = spawn_song_renderer(
				pages.clone(),
				song.version_uuid,
				song.piece_starts
					.keys()
					.map(|&staff| song.staves[staff].page)
					.collect(),
			);
			let width = self.carousel.allocated_width();
			let height = self.carousel.allocated_height();

			update_page.attach(
				None,
				clone_!(self, move |obj, update_page| {
					obj.imp().update_page(update_page);
					Continue(true)
				}),
			);

			self.carousel.grab_focus();

			let song = SongState::new(
				renderer,
				Rc::new(
					std::iter::repeat(Default::default())
						.take(pages.len())
						.collect(),
				),
				song,
				width as f64,
				height as f64,
				scale_mode,
			);

			let parts = song.get_parts();
			self.part_selection.remove_all();
			for (k, p) in &parts {
				self.part_selection.append(Some(&k.to_string()), p);
			}
			let relevant = parts.len() > 1;
			self.part_selection
				.set_active(if relevant { Some(0) } else { None });
			self.part_selection.set_sensitive(relevant);
			self.part_selection.set_visible(relevant);

			self.sizing_mode_action
				.set_state(&scale_mode.action_string().to_variant());

			*self.song.borrow_mut() = Some(song);
			self.instance().notify("song-name");
			self.instance().notify("song-id");

			self.load_annotations();
			self.update_content();
			self.song_progress.get().set_fraction(0.0);

			let now = std::time::Instant::now();
			self.last_interaction.set(now);
			self.song_load_time.set(Some(now));

			/* Scroll to the requested page */
			/* Hack: defer this because of reasons. Also this may be racy */
			let obj = self.instance();
			let carousel = &self.carousel.get();
			glib::MainContext::default().spawn_local(
				clone!(@weak obj, @strong carousel => @default-panic, async move {
					glib::timeout_future(std::time::Duration::from_millis(50)).await;
					let page = obj.imp()
						.song
						.borrow()
						.as_ref()
						.unwrap()
						.layout
						.get_page_of_staff(start_at);
					/* Page count may have changed in the meantime due to race hazards */
					if (*page as u32) < carousel.n_pages() {
						carousel.scroll_to(&carousel.nth_page(*page as u32), false);
					}
				}),
			);
		}

		/// Unload the song
		#[template_callback]
		fn unload_song(&self) {
			let song = self.song.take().unwrap();
			std::mem::drop(song);
			let carousel = &self.carousel;
			for page in (0..carousel.n_pages()).rev() {
				carousel.remove(&carousel.nth_page(page as u32));
			}

			self.part_selection.set_active(None);
			self.part_selection.set_sensitive(false);
			self.part_selection.remove_all();
			self.instance().notify("song-name");
			self.instance().notify("song-id");
			self.on_activity();
			self.song_load_time.take();
		}

		/// The size has changed, maybe update the layout?
		#[template_callback]
		fn on_resize(&self) {
			self.update_content();
			/* Hack: For some reason resizing may use outdated data, therefore force a second update after a few ms */
			let obj = self.instance();
			glib::MainContext::default().spawn_local(
				clone!(@weak obj => @default-panic, async move {
					glib::timeout_future(std::time::Duration::from_millis(5)).await;
					obj.imp().update_content();
				}),
			);
		}

		/// Settings or sizes have changed, update the layout and redraw
		fn update_content(&self) {
			log::debug!("Updating content");
			let carousel = &self.carousel;
			let width = carousel.width();
			let height = carousel.height();

			/* Failsafe against glitches */
			if width <= 1 || height <= 1 {
				return;
			}

			/* Do nothing if no song loaded */
			let mut song_ = self.song.borrow_mut();
			let song = match song_.as_mut() {
				Some(song) => song,
				None => return,
			};

			song.change_size(width as f64, height as f64);

			/* We cannot trigger a resize during a resize, thus use deferred execution */
			let zoom = song.zoom;
			glib::MainContext::default().spawn_local(
				clone!(@strong self.zoom_button as zoom_button => async move {
					zoom_button.set_label(&format!("{:.0}%", zoom * 100.0));
				}),
			);

			/* Update carousel pages, recycle them if possible */
			let new_pages = song.layout.pages.len();
			let old_pages = carousel.n_pages() as usize;
			use std::cmp::Ordering;
			match new_pages.cmp(&old_pages) {
				Ordering::Equal => { /* No page amount change*/ },
				Ordering::Greater => {
					/* Add missing pages */
					for i in old_pages..new_pages {
						let area = crate::song_page::SongPage::new(
							song.song.clone(),
							crate::song_page::PageLayout {
								page: layout::PageIndex(i),
								staves: song.layout.pages[layout::PageIndex(i)].clone(),
								width,
								height,
							},
							song.rendered_pages.clone(),
						);

						carousel.append(&area);
						area.show();
					}
				},
				Ordering::Less => {
					/* Remove excess pages (reverse order is important because of index shifts) */
					for page in (new_pages..old_pages).rev() {
						carousel.remove(&carousel.nth_page(page as u32));
					}
				},
			}
			/* Update existing pages */
			for i in 0..old_pages.min(new_pages) {
				carousel
					.nth_page(i as u32)
					.downcast::<crate::song_page::SongPage>()
					.unwrap()
					.update_layout(crate::song_page::PageLayout {
						page: layout::PageIndex(i),
						staves: song.layout.pages[layout::PageIndex(i)].clone(),
						width,
						height,
					});
			}

			carousel.queue_draw();
			/* Drop song before calling this because nested callbacks */
			let page = *song.page as u32;
			std::mem::drop(song_);
			carousel.scroll_to(&carousel.nth_page(page), false);
		}

		/// The background thread has finished rendering some page
		fn update_page(&self, update_page: ScaledPage) {
			let mut song = self.song.borrow_mut();
			let song = match song.as_mut() {
				Some(song) => song,
				None => return,
			};
			/* Check for stale data */
			if song.song.version_uuid != update_page.song {
				return;
			}

			self.song_progress.get().set_fraction(update_page.progress);
			/* Hide the progress bar automatically after full load */
			if update_page.progress > 0.999 {
				glib::source::timeout_add_local_once(
					std::time::Duration::from_secs(1),
					clone_!(self, move |obj| {
						obj.imp().song_progress.get().set_fraction(0.0);
					}),
				);
			}

			(*song.rendered_pages[update_page.index].borrow_mut()).0 = Some(update_page.image);
			let carousel = &self.carousel;
			for i in 0..carousel.n_pages() {
				carousel.nth_page(i).queue_draw();
			}
			carousel.queue_draw();
		}

		/// When the current carousel page has changed
		#[template_callback]
		fn page_changed(&self, page: u32) {
			/* Do nothing if no song loaded */
			let mut song_ = self.song.borrow_mut();
			let song = match song_.as_mut() {
				Some(song) => song,
				None => return,
			};
			song.change_page(layout::PageIndex(page as usize));
			let active_id = song.part_start(song.page).to_string();

			self.previous_piece.set_enabled(page > 0);
			self.next_piece.set_enabled(
				song.song
					.piece_starts
					.range(song.current_staves.iter().next_back().unwrap()..)
					.next()
					.is_some(),
			);

			std::mem::drop(song_);
			self.part_selection
				.block_signal(self.part_selection_changed_signal.get().unwrap());
			self.part_selection.set_active_id(Some(&active_id));
			self.part_selection
				.unblock_signal(self.part_selection_changed_signal.get().unwrap());
			self.on_activity();
		}

		/// Go to the next page
		fn next_page(&self) {
			let carousel = &self.carousel;
			if self.song.borrow().is_some() {
				let new_page = u32::min(
					carousel.position().round() as u32 + 1,
					carousel.n_pages() as u32 - 1,
				);
				carousel.scroll_to(&carousel.nth_page(new_page), true);
			}
		}

		/// Go to the previous page
		fn previous_page(&self) {
			let carousel = &self.carousel;
			if let Some(song) = self.song.borrow().as_ref() {
				let new_page = song
					.go_back(layout::PageIndex(carousel.position().round() as usize))
					.unwrap_or_else(|| {
						layout::PageIndex(usize::max(carousel.position() as usize, 1) - 1)
					});
				carousel.scroll_to(&carousel.nth_page(*new_page as u32), true);
			}
		}

		/// Key press on the drawingarea
		#[template_callback]
		fn carousel_key(&self, keyval: gdk::Key) -> gtk::Inhibit {
			if keyval == gdk::Key::Left || keyval == gdk::Key::KP_Left {
				self.previous.activate(None);
				gtk::Inhibit(true)
			} else if keyval == gdk::Key::Right || keyval == gdk::Key::KP_Right {
				self.next.activate(None);
				gtk::Inhibit(true)
			} else {
				gtk::Inhibit(false)
			}
		}

		/// Go to beginning of the current or previous piece
		fn previous_piece(&self) {
			let carousel = &self.carousel;
			if let Some(song) = self.song.borrow_mut().as_mut() {
				let staff = song.current_staves[0] - 1.into();
				let (previous_piece_staff, _) = song
					.song
					.piece_starts
					.range(..=staff)
					.next_back()
					.expect("That button should have been disabled");
				let page = *song.layout.get_page_of_staff(*previous_piece_staff);

				carousel.scroll_to(&carousel.nth_page(page as u32), true);
			}
		}

		/// Go to the beginning of the next piece
		fn next_piece(&self) {
			let carousel = &self.carousel;
			if let Some(song) = self.song.borrow_mut().as_mut() {
				let staff = song.current_staves.iter().next_back().unwrap();
				let (next_piece_staff, _) = song
					.song
					.piece_starts
					.range(staff..)
					.next()
					.expect("That button should have been disabled");
				let page = *song.layout.get_page_of_staff(*next_piece_staff);

				carousel.scroll_to(&carousel.nth_page(page as u32), true);
			}
		}

		/// Part got selected from the dropdown, jump to it
		#[template_callback]
		fn select_part(&self) {
			let carousel = &self.carousel;
			if let Some(song) = self.song.borrow().as_ref() {
				let section = self.part_selection.active_id().unwrap();
				let page = *song
					.layout
					.get_page_of_staff(section.parse::<collection::StaffIndex>().unwrap());
				carousel.scroll_to(&carousel.nth_page(page as u32), true);
			}
		}

		/* Events from the zoom gesture */
		#[template_callback]
		fn zoom_gesture_start(&self) {
			log::debug!("Zoom begin");
			if let Some(song) = self.song.borrow_mut().as_mut() {
				song.zoom_before_gesture = Some(song.zoom);
			}
		}

		#[template_callback]
		fn zoom_gesture_end(&self) {
			log::debug!("Zoom end");
			if let Some(song) = self.song.borrow_mut().as_mut() {
				song.zoom_before_gesture = None;
			}
		}

		#[template_callback]
		fn zoom_gesture_cancel(&self) {
			log::debug!("Zoom cancel");
			self.update_manual_zoom(|song| {
				song.zoom_before_gesture
					.take()
					.expect("Should always be Some within after gesture started")
				// .unwrap_or(song.zoom)
			});
		}

		#[template_callback]
		fn zoom_gesture_update(&self, scale: f64) {
			self.update_manual_zoom(|song| {
				let zoom = scale
					* song
						.zoom_before_gesture
						.expect("Should always be Some within after gesture started");
				zoom.clamp(0.6, 3.0)
			});
		}

		/// One zoom in increment
		fn zoom_in(&self) {
			self.update_manual_zoom(|song| (song.zoom / 0.95).clamp(0.6, 3.0));
		}
		/// One zoom out increment
		fn zoom_out(&self) {
			self.update_manual_zoom(|song| (song.zoom * 0.95).clamp(0.6, 3.0));
		}
		/// Set zoom back to 100%
		fn zoom_reset(&self) {
			self.update_manual_zoom(|_| 1.0);
		}

		fn update_manual_zoom(&self, modify_zoom: impl FnOnce(&mut SongState) -> f64) {
			if let Some(song) = self.song.borrow_mut().as_mut() {
				song.zoom = modify_zoom(song);
				song.scale_mode = ScaleMode::Zoom(song.zoom as f32);
			}
			self.sizing_mode_action.set_state(&"manual".to_variant());
			self.update_content();
			self.on_activity();
		}

		pub(super) fn scale_mode_changed(&self, mode: &glib::Variant) {
			/* Idempotent if the signal came from the action itself */
			self.sizing_mode_action.set_state(mode);
			if let Some(song) = self.song.borrow_mut().as_mut() {
				song.scale_mode = match mode.get::<String>().unwrap().as_str() {
					"fit-staves" => ScaleMode::FitStaves(3),
					"fit-columns" => ScaleMode::FitPages(2),
					"manual" => return,
					invalid => unreachable!("Invalid value: '{}'", invalid),
				};
			}
			self.update_content();
			self.on_activity();
		}

		fn stop_cursor_timer(&self) {
			self.instance().set_cursor(None);
			if let Some(hide_cursor) = self.hide_cursor.borrow_mut().take() {
				hide_cursor.remove();
			}
		}

		fn restart_cursor_timer(&self) {
			self.stop_cursor_timer();
			let obj = self.instance();
			*self.hide_cursor.borrow_mut() = Some(glib::source::timeout_add_local_once(
				std::time::Duration::from_secs(4),
				move || {
					obj.imp().hide_cursor.borrow_mut().take();
					obj.set_cursor_from_name(Some("none"));
				},
			));
			self.on_activity();
		}

		/// Should be called on every user action. Update the time played statistic
		fn on_activity(&self) {
			let last_interaction = std::time::Instant::now();
			let diff = last_interaction
				.duration_since(self.last_interaction.get())
				.as_secs()
				/* Consider everything about 3 minutes as "idle" */
				.min(180);
			/* Don't update too often */
			if diff < 5 {
				return;
			}
			let mut song_ = self.song.borrow_mut();
			let song = match song_.as_mut() {
				Some(song) => song,
				None => return,
			};

			let library = &mut self.library.get().unwrap().borrow_mut();
			let stats = library.stats.get_mut(&song.song.song_uuid).unwrap();
			stats.on_update(diff);
			stats.scale_options = Some(song.scale_mode);

			if let Some(song_load_time) = self.song_load_time.get() {
				/* Only register the song as played after 90 seconds */
				if last_interaction.duration_since(song_load_time).as_secs() > 90 {
					library
						.stats
						.get_mut(&song.song.song_uuid)
						.unwrap()
						.on_load();
					self.song_load_time.take();
				}
			}
			library.save_in_background();

			self.last_interaction.set(last_interaction);
		}

		/* Focus on click */
		#[template_callback]
		fn carousel_button_press(&self, _n_press: i32, _x: f64, _y: f64) {
			self.carousel.grab_focus();
		}

		#[template_callback]
		fn carousel_button_release(&self, _n_press: i32, x: f64, _y: f64) {
			let x = x / self.carousel.width() as f64;
			if (0.0..0.3).contains(&x) {
				self.previous.activate(None);
			} else if (0.7..1.0).contains(&x) {
				self.next.activate(None);
			}
		}

		/* Scroll events on the page, for zooming */
		#[template_callback]
		fn carousel_scroll(&self, _dx: f64, dy: f64) -> gtk::Inhibit {
			if self
				.scroll_gesture
				.current_event_state()
				.contains(gdk::ModifierType::CONTROL_MASK)
			{
				self.update_manual_zoom(|song| {
					let zoom = if dy > 0.0 {
						song.zoom * 0.95
					} else {
						song.zoom / 0.95
					};
					zoom.clamp(0.6, 3.0)
				});
				gtk::Inhibit(true)
			} else {
				gtk::Inhibit(false)
			}
		}

		fn load_annotations(&self) {
			if let Some(song) = &self.song.borrow_mut().as_mut() {
				log::debug!("Reloading annotations");
				let uuid = song.song.song_uuid;
				// TODO don't hardcode here
				let xdg = xdg::BaseDirectories::with_prefix("dinoscore").unwrap();
				let annotations_export = xdg
					.place_data_file(format!("annotations/{}.pdf", uuid))
					.unwrap();

				let document = annotations_export.exists().then(|| {
					poppler::Document::from_bytes(
						&glib::Bytes::from_owned(std::fs::read(annotations_export).unwrap()),
						None,
					)
					.unwrap()
				});
				for i in 0..song.rendered_pages.len() {
					(*song.rendered_pages[collection::PageIndex(i)].borrow_mut()).1 =
						document.as_ref().map(|document| {
							document.page(i as i32).expect(
								"Annotation document must have as many pages as original PDF",
							)
						});
				}
				let carousel = &self.carousel;
				for i in 0..carousel.n_pages() {
					carousel.nth_page(i).queue_draw();
				}
				self.carousel.queue_draw();
			}
		}

		/// Launch Xournal++ for annotating
		#[template_callback]
		fn annotate(&self) {
			log::debug!("annotate!");
			if let Some(song) = &self.song.borrow_mut().as_mut() {
				let library = &mut self.library.get().unwrap().borrow_mut();
				let page = song.song.staves[song.current_staves[0]].page;
				let song = library.songs.get_mut(&song.song.song_uuid).unwrap();

				// TODO make async
				// TODO error handling
				use anyhow::Context;
				crate::xournal::run_editor(song, *page + 1)
					.context("Failed to launch editor")
					.unwrap();
			}
			self.load_annotations();
		}
	}
}

struct SongState {
	song: Arc<collection::SongMeta>,
	page: layout::PageIndex,
	layout: Arc<layout::PageLayout>,
	renderer: Sender<(collection::PageIndex, Option<i32>)>,
	rendered_pages:
		Rc<TiVec<collection::PageIndex, RefCell<(Option<gdk::Texture>, Option<poppler::Page>)>>>,
	zoom: f64,
	scale_mode: ScaleMode,
	/* Backup for when a gesture starts */
	zoom_before_gesture: Option<f64>,
	/* For each explicit page turn, track the visible staves. Use that to
	 * synchronize the view on layout changes
	 */
	current_staves: Vec<collection::StaffIndex>,
}

impl SongState {
	fn new(
		renderer: Sender<(collection::PageIndex, Option<i32>)>,
		rendered_pages: Rc<
			TiVec<collection::PageIndex, RefCell<(Option<gdk::Texture>, Option<poppler::Page>)>>,
		>,
		song: Arc<collection::SongMeta>,
		width: f64,
		height: f64,
		scale_mode: ScaleMode,
	) -> Self {
		// let layout = Arc::new(layout::layout_fixed_width(&song, width, height, 1.0, 10.0));
		// let layout = Arc::new(layout::layout_fixed_height(&song, width, height));
		let layout = Arc::new(layout::layout_fixed_scale(&song, width, height, 1.0));
		Self {
			song,
			page: 0.into(),
			current_staves: layout.get_staves_of_page(0.into()).collect(),
			layout,
			renderer,
			rendered_pages,
			zoom: 1.0,
			scale_mode,
			zoom_before_gesture: None,
		}
	}

	fn change_size(&mut self, width: f64, height: f64) {
		// self.layout = Arc::new(layout::layout_fixed_width(&self.song, width, height, zoom, 10.0));
		// self.layout = Arc::new(layout::layout_fixed_height(&self.song, width, height));
		match self.scale_mode {
			ScaleMode::Zoom(_) => {},
			ScaleMode::FitStaves(num) => {
				self.zoom = layout::find_scale_for_fixed_staves(&self.song, width, height, num)
			},
			ScaleMode::FitPages(num) => {
				self.zoom = layout::find_scale_for_fixed_columns(&self.song, width, height, num)
			},
		}

		self.layout = Arc::new(layout::layout_fixed_scale(
			&self.song, width, height, self.zoom,
		));
		/* Calculate the new page, which has the most staves in common with the previous layout/page */
		self.page = {
			use itertools::Itertools;

			self.current_staves
				.iter()
				.copied()
				.map(|staff| self.layout.get_page_of_staff(staff))
				.counts()
				.iter()
				.max_by(|(a_page, a_count), (b_page, b_count)| {
					/* We want smallest page with the most number of hits */
					a_count
						.cmp(b_count)
						.then_with(|| a_page.cmp(b_page).reverse())
				})
				.map(|(page, _count)| *page)
				.unwrap()
		};
		self.current_staves = self.layout.get_staves_of_page(self.page).collect();

		/* Notify background renderer about potential changes */
		self.renderer
			.send((
				/* Convert current layout page to PDF page */
				self.song.staves[self.layout.get_staves_of_page(self.page).next().unwrap()].page,
				Some(width as i32),
			))
			.unwrap();
	}

	fn change_page(&mut self, page: layout::PageIndex) {
		self.page = page;
		self.current_staves = self.layout.get_staves_of_page(page).collect();

		/* Notify background renderer about potential changes */
		self.renderer
			.send((
				/* Convert current layout page to PDF page */
				self.song.staves[self.layout.get_staves_of_page(self.page).next().unwrap()].page,
				None,
			))
			.unwrap();
	}

	fn get_parts(&self) -> Vec<(collection::StaffIndex, String)> {
		self.song
			.piece_starts
			.iter()
			.map(|(k, v)| {
				(
					*k,
					v.is_empty()
						.then(|| format!("({})", k))
						.unwrap_or_else(|| v.clone()),
				)
			})
			.collect()
	}

	/* When we're at a given page and want to go back, should we jump to the start of the repetition? */
	fn go_back(&self, work_page: layout::PageIndex) -> Option<layout::PageIndex> {
		/* Find all sections that are repetitions and are visible on the current page.
		 * Go back to the beginning of the first of them.
		 */
		self.song
			.sections()
			.iter()
			.filter(|(_, repetition)| *repetition)
			.map(|(range, _)| range)
			/* Find a section that ends on the current page but starts somewhere before */
			.find(|range| {
				self.layout.get_page_of_staff(*range.end()) == work_page
					&& self.layout.get_page_of_staff(*range.start()) < work_page
			})
			.map(|range| self.layout.get_page_of_staff(*range.start()))
	}

	/* When we're at a given position, where did the part we are in start? */
	fn part_start(&self, work_page: layout::PageIndex) -> collection::StaffIndex {
		self.song
			.piece_starts
			.iter()
			.filter_map(|(part, _)| {
				if self.layout.get_page_of_staff(*part) <= work_page {
					Some(*part)
				} else {
					None
				}
			})
			.max()
			.unwrap_or_else(|| 0.into())
	}
}

/// A pre-rasterized page
#[derive(Debug)]
struct ScaledPage {
	index: collection::PageIndex,
	image: gdk::Texture,
	/* To filter out old/stale values */
	song: uuid::Uuid,
	progress: f64,
}

/// A background thread renderer
///
/// It will take the raw PDFs and images and render them scaled down to an appropriate
/// size. It is flexible with in-flight requests and invalidation.
///
/// Drop one of the channels when you are no longer interested in that song.
fn spawn_song_renderer(
	pages: Arc<TiVec<collection::PageIndex, PageImage>>,
	song: uuid::Uuid,
	mut piece_starts: Vec<collection::PageIndex>,
) -> (
	Sender<(collection::PageIndex, Option<i32>)>,
	glib::Receiver<ScaledPage>,
) {
	/* Sometimes, two pieces start on the same page. Irrelevant for our purposes */
	piece_starts.dedup();

	let (in_tx, in_rx) = channel();
	let (out_tx, out_rx) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);

	std::thread::spawn(move || {
		use std::collections::VecDeque;
		/* This used to create a simple list of all staves in order.
		 * Except for the initial load, the order does not matter, since
		 * the queue is reordered according to the currently visible page.
		 * Here, we interleave the pages across different parts so that
		 * the users gets a quick initial response, even when jumping directly
		 * to one of the later sections of the song.
		 */
		let reset_work_queue = || {
			let mut piece_starts: Vec<std::ops::Range<collection::PageIndex>> = piece_starts
				.windows(2)
				.map(|win| (win[0], win[1]))
				.map(|(start, end)| start..end)
				.chain(std::iter::once(
					piece_starts[piece_starts.len() - 1]..collection::PageIndex(pages.len()),
				))
				.collect();
			let mut work_queue = VecDeque::with_capacity(pages.len());
			while !piece_starts.is_empty() {
				for piece in &mut piece_starts {
					work_queue.push_back(piece.start);
					piece.start += collection::PageIndex(1);
				}
				piece_starts.retain(|r| !r.is_empty());
			}
			assert_eq!(work_queue.len(), pages.len());
			work_queue
		};

		/* For a start, render everything sequentially at minimum resolution. This should not take long */
		let start = std::time::Instant::now();
		for i in reset_work_queue() {
			let image = gdk::Texture::for_pixbuf(&pages[i].render_scaled(250));
			if out_tx
				.send(ScaledPage {
					index: i,
					image,
					song,
					progress: i.0 as f64 / pages.len() as f64 / pages.len() as f64,
				})
				.is_err()
			{
				return;
			}

			/* Limit the initial step to one second. Otherwise it will take too long to render the first
			 * full resolution image
			 */
			if start.elapsed() > std::time::Duration::from_secs(1) {
				break;
			}
		}
		log::debug!("Background renderer ready");

		/* Start with empty queue since we just already did that resolution */
		let mut work_queue = VecDeque::default();
		let mut work_width = 250;
		let mut work_page = collection::PageIndex(0);

		/* We always only want the latest value */
		type Update = (collection::PageIndex, Option<i32>);
		fn fetch_latest(rx: &Receiver<Update>, block: bool) -> Result<Option<Update>, ()> {
			let mut last = None::<Update>;
			loop {
				match rx.try_recv() {
					Ok((page, None)) if last.is_some() => {
						last = Some((page, last.unwrap().1));
					},
					Ok(val) => {
						last = Some(val);
					},
					Err(TryRecvError::Empty) if last.is_none() && block => {
						/* Don't return empty handed */
						return rx.recv().map(Option::Some).map_err(|_| ());
					},
					Err(TryRecvError::Empty) => return Ok(last),
					Err(TryRecvError::Disconnected) => return Err(()),
				}
			}
		}

		loop {
			let mut need_queue_shuffle = false;

			/* If we have work to do, we simply check for potential invalidation. If we're idle, block on new work */
			match fetch_latest(&in_rx, work_queue.is_empty()) {
				Ok(Some((page, width))) => {
					/* Change for width changes */
					if let Some(width) = width {
						/* Round the width to the nearest level. Never round down, never round more than
						 * 66% (the levels are 2/3 apart each, exponentially). Never go below 250 pixels.
						 */
						let actual_width = (1.5f64)
							.powf((width as f64).log(1.5).ceil())
							.ceil()
							.max(250.0) as i32;
						if actual_width != work_width {
							log::debug!(
								"Background thread rendering width changed: {actual_width}"
							);
							work_width = actual_width;
							work_queue = reset_work_queue();
							need_queue_shuffle = true;
						}
					}

					/* Check for page changes, update work queue accordingly */
					if page != work_page {
						work_page = page;
						need_queue_shuffle = true;
					}
				},
				Ok(None) => (),
				Err(_) => return,
			}

			/* Update queue based on distance to the current page */
			if need_queue_shuffle && !work_queue.is_empty() {
				log::debug!("Priority page change: {work_page}");
				work_queue
					.make_contiguous()
					.sort_unstable_by_key(|page| (**page as isize - *work_page as isize).abs());
			}

			if let Some(page) = work_queue.pop_front() {
				/* Now we can finally do some work */
				let image = gdk::Texture::for_pixbuf(&pages[page].render_scaled(work_width));

				/* Send it off */
				if out_tx
					.send(ScaledPage {
						index: page,
						image,
						song,
						progress: (pages.len() - work_queue.len()) as f64 / pages.len() as f64,
					})
					.is_err()
				{
					return;
				}
			}
		}
	});

	(in_tx, out_rx)
}
