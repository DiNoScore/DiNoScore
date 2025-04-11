use dinoscore::{library::ScaleMode, prelude::*, *};

glib::wrapper! {
	pub struct SongPane(ObjectSubclass<imp::SongPane>)
		@extends gtk::Box, gtk::Widget,
		@implements gtk::Accessible, gtk::Buildable,
					gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl SongPane {
	pub fn init(&self, library: Rc<RefCell<library::Library>>) {
		self.imp().library.set(library).unwrap();
	}

	pub fn load_song(
		&self,
		song: collection::SongMeta,
		pages: TiVec<collection::PageIndex, PageImage>,
		scale_mode: ScaleMode,
		start_at_part: u32,
	) {
		self.imp()
			.load_song(song, Arc::new(pages), scale_mode, start_at_part);
	}

	pub fn unload_song(&self) {
		self.imp().unload_song();
	}

	#[cfg(test)]
	pub fn carousel(&self) -> crate::song_widget::SongWidget {
		self.imp().carousel.get()
	}

	#[cfg(test)]
	pub fn zoom_button(&self) -> gtk::MenuButton {
		self.imp().zoom_button.get()
	}
}

mod imp {
	use super::*;

	#[derive(CompositeTemplate)]
	#[template(resource = "/de/piegames/dinoscore/viewer/song.ui")]
	pub struct SongPane {
		#[template_child]
		header: TemplateChild<adw::HeaderBar>,
		#[template_child]
		pub carousel: TemplateChild<crate::song_widget::SongWidget>,
		#[template_child]
		song_progress: TemplateChild<gtk::ProgressBar>,
		#[template_child]
		pub part_selection: TemplateChild<gtk::DropDown>,
		#[template_child]
		pub zoom_button: TemplateChild<gtk::MenuButton>,

		pub library: OnceCell<Rc<RefCell<library::Library>>>,
		pub song: RefCell<Option<Arc<collection::SongMeta>>>,

		last_interaction: Cell<std::time::Instant>,
		/// Some when loading a song. After 90 seconds, we increment the load count and set to None
		song_load_time: Cell<Option<std::time::Instant>>,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for SongPane {
		const NAME: &'static str = "ViewerSong";
		type Type = super::SongPane;
		type ParentType = gtk::Box;

		fn new() -> Self {
			SongPane {
				header: Default::default(),
				carousel: Default::default(),
				song_progress: Default::default(),
				part_selection: Default::default(),
				zoom_button: Default::default(),
				library: Default::default(),
				song: Default::default(),

				last_interaction: std::time::Instant::now().into(),
				song_load_time: Default::default(),
			}
		}

		fn class_init(klass: &mut Self::Class) {
			klass.bind_template();
			klass.bind_template_callbacks();
		}

		fn instance_init(obj: &InitializingObject<Self>) {
			obj.init_template();
		}
	}

	impl ObjectImpl for SongPane {
		fn properties() -> &'static [glib::ParamSpec] {
			Box::leak(Box::new([
				glib::ParamSpecString::builder("song-name")
					.nick("song-name")
					.blurb("name")
					.default_value(None)
					.flags(glib::ParamFlags::READABLE)
					.build(),
				glib::ParamSpecString::builder("song-id")
					.nick("song-id")
					.blurb("uuid")
					.default_value(None)
					.flags(glib::ParamFlags::READABLE)
					.build(),
			]))
		}

		fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
			match pspec.name() {
				"song-name" => self
					.song
					.borrow()
					.as_ref()
					.and_then(|song| song.title.as_ref())
					.to_value(),
				"song-id" => self
					.song
					.borrow()
					.as_ref()
					.map(|song| song.song_uuid.to_string())
					.to_value(),
				_ => unimplemented!(),
			}
		}

		fn constructed(&self) {
			self.parent_constructed();
			let obj: glib::BorrowedObject<super::SongPane> = self.obj();

			obj.insert_action_group("song", Some(&self.carousel.imp().actions));

			self.carousel.bind_property("zoom", &*self.zoom_button, "label")
				.transform_to(|_, zoom: f64| {
					Some(format!("{:.0}%", zoom * 100.0).to_value())
				})
				.sync_create()
				.build();

			/* MIDI handling */
			#[cfg(unix)]
			{
				let (midi_tx, mut midi_rx) = futures::channel::mpsc::unbounded();
				let handler = crate::pedal::run(midi_tx).unwrap();
				let obj = obj.clone();
				glib::MainContext::default().spawn_local(async move {
					use futures::StreamExt;
					while let Some(event) = midi_rx.next().await {
						/* Reference the MIDI handler which holds the Sender so that it doesn't get dropped. */
						let _handler = &handler;
						match event {
							crate::pedal::PageEvent::Next => {
								obj.imp().carousel.next_page();
							},
							crate::pedal::PageEvent::Previous => {
								obj.imp().carousel.previous_page();
							},
						}
					}
					log::info!("Midi thread exited.");
				});
			}
		}
	}

	impl WidgetImpl for SongPane {}

	impl BoxImpl for SongPane {}

	#[gtk::template_callbacks]
	impl SongPane {
		pub fn load_song(
			&self,
			song: collection::SongMeta,
			pages: Arc<TiVec<collection::PageIndex, PageImage>>,
			scale_mode: ScaleMode,
			start_at_part: u32,
		) {
			let obj = self.obj();

			log::debug!("Loading song");
			log::debug!(
				"UUID: {}, starting at: {}, scale mode: {:?}",
				song.song_uuid,
				start_at_part,
				scale_mode
			);
			let song = Arc::new(song);

			self.carousel.grab_focus();
			self.carousel.load_song(
				&song,
				pages
			);
			self.carousel.set_scale_mode(scale_mode);

			let parts: Vec<(collection::StaffIndex, String)> = song.parts();
			let part_selection_model = self.part_selection.model().unwrap().downcast::<gtk::StringList>().unwrap();
			part_selection_model.splice(
				0,
				part_selection_model.n_items(),
				&*parts.iter().map(|(_, name)| &**name).collect::<Vec<_>>()
			);
			let relevant = parts.len() > 1;
			self.part_selection.set_sensitive(relevant);
			self.part_selection.set_visible(relevant);
			/* Scroll to the requested page */
			self.part_selection.set_selected(start_at_part);

			*self.song.borrow_mut() = Some(song);
			obj.notify("song-name");
			obj.notify("song-id");

			self.load_annotations();
			self.song_progress.get().set_fraction(0.0);

			let now = std::time::Instant::now();
			self.last_interaction.set(now);
			self.song_load_time.set(Some(now));
		}

		/// Unload the song
		#[template_callback]
		pub fn unload_song(&self) {
			let song = self.song.take().unwrap();
			std::mem::drop(song);

			self.carousel.unload_song();
			self.part_selection.set_sensitive(false);
			let part_selection_model = self.part_selection.model().unwrap().downcast::<gtk::StringList>().unwrap();
			part_selection_model.splice(0, part_selection_model.n_items(), &[]);
			self.obj().notify("song-name");
			self.obj().notify("song-id");
			self.on_activity();
			self.song_load_time.take();
		}

		#[template_callback]
		fn page_load_progress(&self, progress: f64) {
			self.song_progress.get().set_fraction(progress);
			/* Hide the progress bar automatically after full load */
			if progress > 0.999 {
				glib::source::timeout_add_local_once(
					std::time::Duration::from_secs(1),
					clone_!(self, move |obj| {
						obj.imp().song_progress.get().set_fraction(0.0);
					}),
				);
			}
		}

		/// Should be called on every user action. Updates the time played statistic
		#[template_callback]
		fn on_activity(&self) {
			let last_interaction = std::time::Instant::now();
			let diff = last_interaction
				.duration_since(self.last_interaction.get())
				.as_secs()
				/* Consider everything above 3 minutes as "idle" */
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
			let stats = library.stats.get_mut(&song.song_uuid).unwrap();
			stats.on_update(diff);
			stats.scale_options = Some(self.carousel.get_scale_mode());

			if let Some(song_load_time) = self.song_load_time.get() {
				/* Only register the song as played after 90 seconds */
				if last_interaction.duration_since(song_load_time).as_secs() > 90 {
					log::debug!("Song now counts as \"played\"");
					self.song_load_time.take();
					library
						.stats
						.get_mut(&song.song_uuid)
						.unwrap()
						.on_load();
				}
			}
			library.save_in_background();

			self.last_interaction.set(last_interaction);
		}

		fn load_annotations(&self) {
			if let Some(song) = &self.song.borrow_mut().as_mut() {
				log::debug!("Reloading annotations");
				let uuid = song.song_uuid;
				// TODO don't hardcode here
				let xdg = xdg::BaseDirectories::with_prefix("dinoscore");
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
				self.carousel.load_annotations(document);
			}
		}

		/// Launch Xournal++ for annotating
		#[template_callback]
		fn annotate(&self) {
			log::debug!("annotate!");
			if let Some(song) = &self.song.borrow_mut().as_mut() {
				let library = &mut self.library.get().unwrap().borrow_mut();
				// TODO reimplement
				// let page = song.song.staves[song.current_staves[0]].page;
				let song = library.songs.get_mut(&song.song_uuid).unwrap();

				// TODO make async
				// TODO error handling
				use anyhow::Context;
				crate::xournal::run_editor(song, 0)
					.context("Failed to launch editor")
					.unwrap();
			}
			self.load_annotations();
			self.carousel.grab_focus();
		}
	}
}
